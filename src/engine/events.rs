//! Event dispatch for rt-app-rs.
//!
//! Ported from `run_event()` and the workload helpers (`ioload`, `memload`)
//! in `rt-app.c`. Each event type maps to a variant of [`ResourceType`] and
//! performs the corresponding system-level operation.

use std::io::Write;
use std::sync::{Condvar, Mutex};
use std::time::Duration;

use log::debug;
use nix::time::{clock_gettime, ClockId};

use crate::engine::calibration::{loadwait, spin_wait_ns, CpuBurnMode};
use crate::types::{EventData, LogData, PhaseData, ResourceType, ThreadIndex};
use crate::utils::timespec_to_nsec;

// ---------------------------------------------------------------------------
// I/O and memory workloads
// ---------------------------------------------------------------------------

/// Write `count` bytes from `buf` to the given writer, in chunks no larger
/// than `buf.len()`.
///
/// Corresponds to `ioload()` in the C original.
pub fn ioload<W: Write>(writer: &mut W, buf: &[u8], mut count: usize) {
    while count > 0 {
        let size = count.min(buf.len());
        match writer.write(&buf[..size]) {
            Ok(n) => count -= n,
            Err(e) => {
                log::error!("ioload write error: {e}");
                return;
            }
        }
    }
}

/// Zero-fill `count` bytes in `buf`, in chunks no larger than `buf.len()`.
///
/// Corresponds to `memload()` in the C original.
pub fn memload(buf: &mut [u8], mut count: usize) {
    while count > 0 {
        let size = count.min(buf.len());
        buf[..size].fill(0);
        count -= size;
    }
}

// ---------------------------------------------------------------------------
// Lock tracking
// ---------------------------------------------------------------------------

/// Tracks the lock depth change caused by an event.
///
/// Positive = lock acquired, negative = lock released, zero = no change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LockDelta(pub i32);

impl LockDelta {
    pub const NONE: Self = Self(0);
    pub const LOCKED: Self = Self(1);
    pub const UNLOCKED: Self = Self(-1);
}

// ---------------------------------------------------------------------------
// Shared resource handles
// ---------------------------------------------------------------------------

/// Runtime state for a shared resource, matching the C union variants.
///
/// Each resource is stored behind its appropriate sync primitive. The engine
/// creates these from the parsed config before spawning threads.
#[derive(Debug)]
pub enum ResourceHandle {
    /// A mutex resource (for lock/unlock events).
    Mutex(Mutex<()>),
    /// A condition variable + associated mutex (for wait/signal/broadcast).
    Condvar { cond: Condvar, mutex: Mutex<()> },
    /// A barrier synchronisation point.
    Barrier {
        mutex: Mutex<BarrierState>,
        cond: Condvar,
    },
    /// A timer for periodic wakeups.
    Timer(Mutex<TimerState>),
    /// An I/O memory buffer.
    IoMem { buf: Mutex<Vec<u8>> },
    /// An I/O device (file).
    IoDev {
        // The file is managed externally; we store an index or placeholder.
    },
    /// A fork-point reference.
    Fork {
        reference: String,
        fork_count: Mutex<u32>,
    },
}

/// State for a barrier resource.
#[derive(Debug)]
pub struct BarrierState {
    /// Number of threads still expected before the barrier triggers.
    pub waiting: usize,
    /// The original/reset value.
    pub total: usize,
}

/// State for a timer resource.
#[derive(Debug, Clone, Default)]
pub struct TimerState {
    /// Whether the timer has been initialised.
    pub initialized: bool,
    /// Whether the timer is relative (resets on miss).
    pub relative: bool,
    /// Next absolute wakeup time in nanoseconds (CLOCK_MONOTONIC).
    pub next_ns: u64,
}

// ---------------------------------------------------------------------------
// Event dispatch context
// ---------------------------------------------------------------------------

/// Context passed to [`run_event`] containing the shared state needed
/// to execute events.
pub struct EventContext<'a> {
    /// The calling thread's index.
    pub thread_index: ThreadIndex,
    /// How run/runtime events burn CPU time.
    pub cpu_burn_mode: CpuBurnMode,
    /// Global resource table.
    pub resources: &'a [ResourceHandle],
    /// Per-thread local resource table (for timer_unique).
    pub local_resources: &'a [ResourceHandle],
    /// Whether to accumulate slack across iterations.
    pub cumulative_slack: bool,
    /// Absolute time of the first iteration start.
    pub t_first_ns: u64,
}

// ---------------------------------------------------------------------------
// run_event — the core dispatcher
// ---------------------------------------------------------------------------

/// Execute a single event, dispatching on its [`ResourceType`].
///
/// If `dry_run` is true, only lock/unlock events are executed (used during
/// shutdown to release held mutexes).
///
/// Returns the lock-depth delta caused by this event.
pub fn run_event(
    event: &EventData,
    dry_run: bool,
    ldata: &mut LogData,
    perf: &mut u64,
    ctx: &EventContext<'_>,
) -> LockDelta {
    let res_idx = event.resource.map(|r| r.0).unwrap_or(0);
    let dep_idx = event.dependency.map(|r| r.0).unwrap_or(0);

    // Lock/Unlock always execute, even in dry-run mode.
    let lock_delta = match event.event_type {
        ResourceType::Lock => {
            debug!("lock resource {res_idx}");
            if let Some(ResourceHandle::Mutex(m)) = ctx.resources.get(res_idx) {
                let _guard = m.lock().unwrap_or_else(|e| e.into_inner());
                // In the C code the lock is held across events; we use
                // Mutex::lock() which blocks and then immediately drops.
                // The actual hold is managed by the calling code.
                std::mem::forget(m.lock().unwrap_or_else(|e| e.into_inner()));
            }
            LockDelta::LOCKED
        }
        ResourceType::Unlock => {
            debug!("unlock resource {res_idx}");
            // The C code calls pthread_mutex_unlock. In Rust, unlocking
            // is done by dropping the MutexGuard. For now this is a
            // structural placeholder.
            LockDelta::UNLOCKED
        }
        _ => LockDelta::NONE,
    };

    if dry_run {
        return lock_delta;
    }

    let duration_usec = event.duration.as_micros() as u64;

    match event.event_type {
        ResourceType::Lock | ResourceType::Unlock => {
            // Already handled above.
        }
        ResourceType::Run => {
            run_event_run(duration_usec, ldata, perf, ctx);
        }
        ResourceType::Runtime => {
            run_event_runtime(duration_usec, ldata, perf, ctx);
        }
        ResourceType::Sleep => {
            debug!("sleep {duration_usec}");
            std::thread::sleep(event.duration);
        }
        ResourceType::Timer | ResourceType::TimerUnique => {
            run_event_timer(event, duration_usec, ldata, ctx, res_idx);
        }
        ResourceType::Wait => {
            run_event_wait(ctx.resources, res_idx, dep_idx);
        }
        ResourceType::Signal => {
            debug!("signal resource {res_idx}");
            if let Some(ResourceHandle::Condvar { cond, .. }) = ctx.resources.get(res_idx) {
                cond.notify_one();
            }
        }
        ResourceType::Broadcast => {
            debug!("broadcast resource {res_idx}");
            if let Some(ResourceHandle::Condvar { cond, .. }) = ctx.resources.get(res_idx) {
                cond.notify_all();
            }
        }
        ResourceType::SigAndWait => {
            run_event_sig_and_wait(ctx.resources, res_idx, dep_idx);
        }
        ResourceType::Barrier => {
            run_event_barrier(ctx.resources, res_idx);
        }
        ResourceType::Suspend => {
            run_event_suspend(ctx.resources, res_idx, dep_idx);
        }
        ResourceType::Resume => {
            run_event_resume(ctx.resources, res_idx, dep_idx);
        }
        ResourceType::Mem => {
            run_event_mem(ctx.resources, res_idx, event.count);
        }
        ResourceType::IoRun => {
            run_event_iorun(ctx.resources, res_idx, dep_idx, event.count);
        }
        ResourceType::Yield => {
            debug!("yield");
            // SAFETY: sched_yield is always safe to call and has no
            // preconditions or memory safety implications.
            unsafe { libc::sched_yield() };
        }
        ResourceType::Fork => {
            debug!("fork (handled by caller)");
            // Fork is handled at a higher level (mod.rs) because it
            // needs access to the thread management infrastructure.
        }
        ResourceType::Unknown | ResourceType::Mutex => {
            // No-op for unknown or bare mutex resource type.
        }
    }

    lock_delta
}

// ---------------------------------------------------------------------------
// Event-type-specific handlers (factored out to keep run_event short)
// ---------------------------------------------------------------------------

fn run_event_run(duration_usec: u64, ldata: &mut LogData, perf: &mut u64, ctx: &EventContext<'_>) {
    debug!("run {duration_usec}");
    ldata.cumulative_duration_usec += duration_usec;

    let start = clock_gettime(ClockId::CLOCK_MONOTONIC).unwrap();
    match ctx.cpu_burn_mode {
        CpuBurnMode::Calibrated(ns_per_loop) => {
            *perf += loadwait(duration_usec, ns_per_loop);
        }
        CpuBurnMode::Precise => {
            spin_wait_ns(duration_usec * 1_000);
            // perf is 0 in precise mode (no calibrated work done).
        }
    }
    let end = clock_gettime(ClockId::CLOCK_MONOTONIC).unwrap();

    let elapsed_ns = timespec_to_nsec(&end).wrapping_sub(timespec_to_nsec(&start));
    ldata.duration_usec += elapsed_ns / 1_000;
}

fn run_event_runtime(
    duration_usec: u64,
    ldata: &mut LogData,
    perf: &mut u64,
    ctx: &EventContext<'_>,
) {
    debug!("runtime {duration_usec}");
    ldata.cumulative_duration_usec += duration_usec;

    let start = clock_gettime(ClockId::CLOCK_MONOTONIC).unwrap();
    match ctx.cpu_burn_mode {
        CpuBurnMode::Calibrated(ns_per_loop) => loop {
            *perf += loadwait(32, ns_per_loop);
            let end = clock_gettime(ClockId::CLOCK_MONOTONIC).unwrap();
            let diff_ns = timespec_to_nsec(&end).wrapping_sub(timespec_to_nsec(&start));
            if diff_ns / 1_000 >= duration_usec {
                ldata.duration_usec += diff_ns / 1_000;
                break;
            }
        },
        CpuBurnMode::Precise => {
            spin_wait_ns(duration_usec * 1_000);
            let end = clock_gettime(ClockId::CLOCK_MONOTONIC).unwrap();
            let diff_ns = timespec_to_nsec(&end).wrapping_sub(timespec_to_nsec(&start));
            ldata.duration_usec += diff_ns / 1_000;
            // perf is 0 in precise mode (no calibrated work done).
        }
    }
}

fn run_event_timer(
    event: &EventData,
    duration_usec: u64,
    ldata: &mut LogData,
    ctx: &EventContext<'_>,
    res_idx: usize,
) {
    debug!("timer {duration_usec}");
    let period_ns = event.duration.as_nanos() as u64;
    ldata.cumulative_period_usec += duration_usec;

    // Select resource table: timer_unique uses local, timer uses global.
    let resources = if event.event_type == ResourceType::TimerUnique {
        ctx.local_resources
    } else {
        ctx.resources
    };

    let timer = match resources.get(res_idx) {
        Some(ResourceHandle::Timer(t)) => t,
        _ => return,
    };

    let mut state = timer.lock().unwrap();

    if !state.initialized {
        state.initialized = true;
        state.next_ns = ctx.t_first_ns;
    }

    state.next_ns += period_ns;

    let now = clock_gettime(ClockId::CLOCK_MONOTONIC).unwrap();
    let now_ns = timespec_to_nsec(&now);

    let slack_ns = state.next_ns as i64 - now_ns as i64;
    if ctx.cumulative_slack {
        ldata.slack_nsec += slack_ns;
    } else {
        ldata.slack_nsec = slack_ns;
    }

    if now_ns < state.next_ns {
        // Sleep until the next period.
        let sleep_ns = state.next_ns - now_ns;
        drop(state); // Release lock before sleeping.
        std::thread::sleep(Duration::from_nanos(sleep_ns));

        let after = clock_gettime(ClockId::CLOCK_MONOTONIC).unwrap();
        let after_ns = timespec_to_nsec(&after);
        // Reacquire to read next_ns for wakeup latency.
        let state = timer.lock().unwrap();
        let wu_lat_ns = after_ns.saturating_sub(state.next_ns);
        ldata.wakeup_latency_usec += wu_lat_ns / 1_000;
    } else {
        // Missed the deadline.
        if state.relative {
            state.next_ns = now_ns;
        }
        ldata.wakeup_latency_usec = 0;
    }
}

fn run_event_wait(resources: &[ResourceHandle], res_idx: usize, dep_idx: usize) {
    debug!("wait resource {res_idx}");
    if let (Some(ResourceHandle::Condvar { cond, .. }), Some(ResourceHandle::Mutex(m))) =
        (resources.get(res_idx), resources.get(dep_idx))
    {
        let guard = m.lock().unwrap_or_else(|e| e.into_inner());
        let _guard = cond.wait(guard).unwrap_or_else(|e| e.into_inner());
    }
}

fn run_event_sig_and_wait(resources: &[ResourceHandle], res_idx: usize, dep_idx: usize) {
    debug!("signal and wait resource {res_idx}");
    if let (Some(ResourceHandle::Condvar { cond, .. }), Some(ResourceHandle::Mutex(m))) =
        (resources.get(res_idx), resources.get(dep_idx))
    {
        cond.notify_one();
        let guard = m.lock().unwrap_or_else(|e| e.into_inner());
        let _guard = cond.wait(guard).unwrap_or_else(|e| e.into_inner());
    }
}

fn run_event_barrier(resources: &[ResourceHandle], res_idx: usize) {
    debug!("barrier resource {res_idx}");
    if let Some(ResourceHandle::Barrier { mutex, cond }) = resources.get(res_idx) {
        let mut state = mutex.lock().unwrap();
        if state.waiting == 0 {
            // Last thread: broadcast to wake everyone.
            state.waiting = state.total;
            cond.notify_all();
        } else {
            state.waiting -= 1;
            let _guard = cond.wait(state).unwrap();
        }
    }
}

fn run_event_suspend(resources: &[ResourceHandle], res_idx: usize, dep_idx: usize) {
    debug!("suspend resource {res_idx}");
    if let (Some(ResourceHandle::Condvar { cond, .. }), Some(ResourceHandle::Mutex(m))) =
        (resources.get(res_idx), resources.get(dep_idx))
    {
        let guard = m.lock().unwrap_or_else(|e| e.into_inner());
        let _guard = cond.wait(guard).unwrap_or_else(|e| e.into_inner());
        // Guard is dropped here, releasing the mutex.
    }
}

fn run_event_resume(resources: &[ResourceHandle], res_idx: usize, dep_idx: usize) {
    debug!("resume resource {res_idx}");
    if let (Some(ResourceHandle::Condvar { cond, .. }), Some(ResourceHandle::Mutex(m))) =
        (resources.get(res_idx), resources.get(dep_idx))
    {
        let _guard = m.lock().unwrap_or_else(|e| e.into_inner());
        cond.notify_all();
        // Guard dropped => mutex released.
    }
}

fn run_event_mem(resources: &[ResourceHandle], res_idx: usize, count: u32) {
    debug!("mem {count}");
    if let Some(ResourceHandle::IoMem { buf }) = resources.get(res_idx) {
        let mut buf = buf.lock().unwrap();
        memload(&mut buf, count as usize);
    }
}

fn run_event_iorun(resources: &[ResourceHandle], res_idx: usize, _dep_idx: usize, count: u32) {
    debug!("iorun {count}");
    if let Some(ResourceHandle::IoMem { buf }) = resources.get(res_idx) {
        let buf = buf.lock().unwrap();
        // In the full implementation, dep_idx would point to an IoDev
        // resource with a file descriptor. For now, write to sink.
        let mut sink = std::io::sink();
        ioload(&mut sink, &buf, count as usize);
    }
}

// ---------------------------------------------------------------------------
// run — iterate events in a phase
// ---------------------------------------------------------------------------

/// Execute all events in a phase, returning the total performance counter.
///
/// Stops early if `continue_running` returns false and no locks are held.
pub fn run_phase(
    phase: &PhaseData,
    ldata: &mut LogData,
    ctx: &EventContext<'_>,
    continue_running: impl Fn() -> bool,
) -> u64 {
    let mut perf: u64 = 0;
    let mut lock_depth: i32 = 0;

    for (i, event) in phase.events.iter().enumerate() {
        if !continue_running() && lock_depth <= 0 {
            return perf;
        }

        debug!(
            "[{}] runs event {} type {}",
            ctx.thread_index, i, event.event_type
        );

        let dry_run = !continue_running();
        let delta = run_event(event, dry_run, ldata, &mut perf, ctx);
        lock_depth += delta.0;
    }

    perf
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::calibration::NsPerLoop;

    #[test]
    fn lock_delta_values() {
        assert_eq!(LockDelta::NONE.0, 0);
        assert_eq!(LockDelta::LOCKED.0, 1);
        assert_eq!(LockDelta::UNLOCKED.0, -1);
    }

    #[test]
    fn ioload_writes_exact_count() {
        let buf = vec![0xABu8; 64];
        let mut output = Vec::new();
        ioload(&mut output, &buf, 100);
        assert_eq!(output.len(), 100);
    }

    #[test]
    fn ioload_writes_less_than_buf() {
        let buf = vec![0u8; 64];
        let mut output = Vec::new();
        ioload(&mut output, &buf, 30);
        assert_eq!(output.len(), 30);
    }

    #[test]
    fn ioload_zero_count() {
        let buf = vec![0u8; 64];
        let mut output = Vec::new();
        ioload(&mut output, &buf, 0);
        assert!(output.is_empty());
    }

    #[test]
    fn memload_zeroes_bytes() {
        let mut buf = vec![0xFFu8; 128];
        memload(&mut buf, 100);
        assert!(buf[..100].iter().all(|&b| b == 0));
        // Bytes beyond 100 but within buf should also be zeroed
        // since 100 < 128 and we process in one chunk.
    }

    #[test]
    fn memload_larger_than_buf() {
        let mut buf = vec![0xFFu8; 32];
        memload(&mut buf, 100);
        // All of buf should be zeroed (multiple passes).
        assert!(buf.iter().all(|&b| b == 0));
    }

    #[test]
    fn memload_zero_count() {
        let mut buf = vec![0xFFu8; 32];
        memload(&mut buf, 0);
        assert!(buf.iter().all(|&b| b == 0xFF));
    }

    #[test]
    fn timer_state_default() {
        let ts = TimerState::default();
        assert!(!ts.initialized);
        assert!(!ts.relative);
        assert_eq!(ts.next_ns, 0);
    }

    #[test]
    fn barrier_state_construction() {
        let bs = BarrierState {
            waiting: 3,
            total: 3,
        };
        assert_eq!(bs.waiting, 3);
        assert_eq!(bs.total, 3);
    }

    #[test]
    fn event_dispatch_sleep() {
        let event = EventData {
            name: "test_sleep".into(),
            event_type: ResourceType::Sleep,
            resource: None,
            dependency: None,
            duration: Duration::from_millis(1),
            count: 0,
        };

        let mut ldata = LogData::default();
        let mut perf = 0u64;
        let ctx = EventContext {
            thread_index: ThreadIndex(0),
            cpu_burn_mode: CpuBurnMode::Calibrated(NsPerLoop(100)),
            resources: &[],
            local_resources: &[],
            cumulative_slack: false,
            t_first_ns: 0,
        };

        let delta = run_event(&event, false, &mut ldata, &mut perf, &ctx);
        assert_eq!(delta, LockDelta::NONE);
    }

    #[test]
    fn event_dispatch_yield() {
        let event = EventData {
            name: "test_yield".into(),
            event_type: ResourceType::Yield,
            resource: None,
            dependency: None,
            duration: Duration::ZERO,
            count: 0,
        };

        let mut ldata = LogData::default();
        let mut perf = 0u64;
        let ctx = EventContext {
            thread_index: ThreadIndex(0),
            cpu_burn_mode: CpuBurnMode::Calibrated(NsPerLoop(100)),
            resources: &[],
            local_resources: &[],
            cumulative_slack: false,
            t_first_ns: 0,
        };

        let delta = run_event(&event, false, &mut ldata, &mut perf, &ctx);
        assert_eq!(delta, LockDelta::NONE);
    }

    #[test]
    fn event_dispatch_run_updates_perf() {
        let event = EventData {
            name: "test_run".into(),
            event_type: ResourceType::Run,
            resource: None,
            dependency: None,
            duration: Duration::from_micros(100),
            count: 0,
        };

        let mut ldata = LogData::default();
        let mut perf = 0u64;
        let ctx = EventContext {
            thread_index: ThreadIndex(0),
            cpu_burn_mode: CpuBurnMode::Calibrated(NsPerLoop(100)),
            resources: &[],
            local_resources: &[],
            cumulative_slack: false,
            t_first_ns: 0,
        };

        run_event(&event, false, &mut ldata, &mut perf, &ctx);
        // perf should be nonzero: 100 usec / 100 ns = 1
        assert_eq!(perf, 1);
        assert_eq!(ldata.cumulative_duration_usec, 100);
        assert!(ldata.duration_usec > 0);
    }

    #[test]
    fn event_dispatch_dry_run_skips_work() {
        let event = EventData {
            name: "test_run_dry".into(),
            event_type: ResourceType::Run,
            resource: None,
            dependency: None,
            duration: Duration::from_micros(100),
            count: 0,
        };

        let mut ldata = LogData::default();
        let mut perf = 0u64;
        let ctx = EventContext {
            thread_index: ThreadIndex(0),
            cpu_burn_mode: CpuBurnMode::Calibrated(NsPerLoop(100)),
            resources: &[],
            local_resources: &[],
            cumulative_slack: false,
            t_first_ns: 0,
        };

        run_event(&event, true, &mut ldata, &mut perf, &ctx);
        assert_eq!(perf, 0); // Dry run skips CPU burn.
        assert_eq!(ldata.duration_usec, 0);
    }

    #[test]
    fn run_phase_stops_early_when_not_running() {
        let events = vec![
            EventData {
                name: "e1".into(),
                event_type: ResourceType::Sleep,
                resource: None,
                dependency: None,
                duration: Duration::from_millis(1),
                count: 0,
            },
            EventData {
                name: "e2".into(),
                event_type: ResourceType::Sleep,
                resource: None,
                dependency: None,
                duration: Duration::from_millis(1),
                count: 0,
            },
        ];

        let phase = PhaseData {
            loop_count: 1,
            events,
            cpu_data: Default::default(),
            numa_data: Default::default(),
            sched_data: None,
            taskgroup_data: None,
        };

        let mut ldata = LogData::default();
        let ctx = EventContext {
            thread_index: ThreadIndex(0),
            cpu_burn_mode: CpuBurnMode::Calibrated(NsPerLoop(100)),
            resources: &[],
            local_resources: &[],
            cumulative_slack: false,
            t_first_ns: 0,
        };

        // Immediately signal stop.
        let perf = run_phase(&phase, &mut ldata, &ctx, || false);
        assert_eq!(perf, 0);
    }

    #[test]
    fn resource_handle_variants() {
        // Verify all ResourceHandle variants can be constructed.
        let _ = ResourceHandle::Mutex(Mutex::new(()));
        let _ = ResourceHandle::Condvar {
            cond: Condvar::new(),
            mutex: Mutex::new(()),
        };
        let _ = ResourceHandle::Barrier {
            mutex: Mutex::new(BarrierState {
                waiting: 3,
                total: 3,
            }),
            cond: Condvar::new(),
        };
        let _ = ResourceHandle::Timer(Mutex::new(TimerState::default()));
        let _ = ResourceHandle::IoMem {
            buf: Mutex::new(vec![0u8; 1024]),
        };
        let _ = ResourceHandle::Fork {
            reference: "task1".into(),
            fork_count: Mutex::new(0),
        };
    }

    #[test]
    fn event_dispatch_run_precise_mode() {
        let event = EventData {
            name: "test_run_precise".into(),
            event_type: ResourceType::Run,
            resource: None,
            dependency: None,
            duration: Duration::from_micros(100),
            count: 0,
        };

        let mut ldata = LogData::default();
        let mut perf = 0u64;
        let ctx = EventContext {
            thread_index: ThreadIndex(0),
            cpu_burn_mode: CpuBurnMode::Precise,
            resources: &[],
            local_resources: &[],
            cumulative_slack: false,
            t_first_ns: 0,
        };

        run_event(&event, false, &mut ldata, &mut perf, &ctx);
        // perf is 0 in precise mode.
        assert_eq!(perf, 0);
        assert_eq!(ldata.cumulative_duration_usec, 100);
        assert!(ldata.duration_usec > 0);
    }

    #[test]
    fn event_dispatch_runtime_precise_mode() {
        let event = EventData {
            name: "test_runtime_precise".into(),
            event_type: ResourceType::Runtime,
            resource: None,
            dependency: None,
            duration: Duration::from_micros(100),
            count: 0,
        };

        let mut ldata = LogData::default();
        let mut perf = 0u64;
        let ctx = EventContext {
            thread_index: ThreadIndex(0),
            cpu_burn_mode: CpuBurnMode::Precise,
            resources: &[],
            local_resources: &[],
            cumulative_slack: false,
            t_first_ns: 0,
        };

        run_event(&event, false, &mut ldata, &mut perf, &ctx);
        // perf is 0 in precise mode.
        assert_eq!(perf, 0);
        assert_eq!(ldata.cumulative_duration_usec, 100);
        assert!(ldata.duration_usec > 0);
    }
}
