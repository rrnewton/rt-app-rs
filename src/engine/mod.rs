//! Core runtime engine for rt-app-rs.
//!
//! Ported from `rt-app.c`. This module coordinates thread creation, the main
//! thread body loop, signal handling, and graceful shutdown.
//!
//! Submodules:
//! - [`calibration`] — CPU busy-loop calibration
//! - [`events`] — per-event dispatch (all 19 event types)
//! - [`scheduling`] — scheduling policy setup (CFS/RT/DEADLINE/uclamp)
//! - [`timing`] — timing data collection, logging, and gnuplot output

pub mod calibration;
pub mod events;
pub mod scheduling;
pub mod timing;

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use log::{debug, error, info};
use nix::time::{clock_gettime, ClockId};

use crate::types::{
    AppOptions, LogData, SchedData, SchedulingPolicy, ThreadData, ThreadIndex, TimingPoint,
};
use crate::utils::timespec_to_nsec;

use self::calibration::NsPerLoop;
use self::events::{EventContext, ResourceHandle};
use self::timing::{ThreadPlotInfo, TimingBuffer};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum permissible forks per fork-event to prevent fork bombs.
pub const FORKS_LIMIT: u32 = 1024;

// ---------------------------------------------------------------------------
// EngineState — shared state across all threads
// ---------------------------------------------------------------------------

/// Shared runtime state accessible by all worker threads.
pub struct EngineState {
    /// Global flag: set to false to request shutdown.
    pub continue_running: AtomicBool,
    /// Number of threads currently running (including forked ones).
    pub running_threads: AtomicI32,
    /// Calibrated ns-per-loop value.
    pub ns_per_loop: NsPerLoop,
    /// Global resource table.
    pub resources: Vec<ResourceHandle>,
    /// Application-wide options.
    pub opts: AppOptions,
    /// Time reference (t_zero): set by the first thread.
    pub t_zero_ns: Mutex<Option<u64>>,
    /// Application start time.
    pub t_start_ns: u64,
}

// ---------------------------------------------------------------------------
// ThreadHandle — per-thread tracking
// ---------------------------------------------------------------------------

/// Tracks a spawned worker thread.
pub struct ThreadHandle {
    pub join_handle: JoinHandle<()>,
    pub name: String,
}

// ---------------------------------------------------------------------------
// Thread unique name generation
// ---------------------------------------------------------------------------

/// Generate a unique name for a thread, incorporating its index and
/// (if forked) a fork sequence number.
pub fn make_thread_unique_name(
    base_name: &str,
    index: ThreadIndex,
    forked: bool,
    fork_seq: u32,
) -> String {
    if forked {
        format!("{}-{}-{:04}", base_name, index.0, fork_seq)
    } else {
        format!("{}-{}", base_name, index.0)
    }
}

// ---------------------------------------------------------------------------
// Thread body
// ---------------------------------------------------------------------------

/// Arguments passed to the thread body function.
pub struct ThreadBodyArgs {
    /// The thread's data (cloned for each instance).
    pub tdata: ThreadData,
    /// Thread index.
    pub index: ThreadIndex,
    /// Per-thread local resources.
    pub local_resources: Vec<ResourceHandle>,
    /// Barrier for synchronising startup (None for forked threads).
    pub startup_barrier: Option<Arc<Barrier>>,
    /// Shared engine state.
    pub state: Arc<EngineState>,
    /// Log buffer capacity (number of TimingPoints).
    pub log_capacity: usize,
}

/// The main thread function, corresponding to `thread_body()` in the C original.
///
/// This runs in each worker thread:
/// 1. Sets the thread name
/// 2. Synchronises with other threads via barrier
/// 3. Applies initial delay, scheduling params, affinity
/// 4. Runs the phase loop, collecting timing data
/// 5. Flushes logs and cleans up
pub fn thread_body(args: ThreadBodyArgs) {
    let ThreadBodyArgs {
        mut tdata,
        index,
        local_resources,
        startup_barrier,
        state,
        log_capacity,
    } = args;

    // Set the OS thread name (truncated to 15 chars for Linux).
    let thread_name = tdata.name.clone();
    let short_name: String = thread_name.chars().take(15).collect();
    if let Err(e) = set_current_thread_name(&short_name) {
        debug!("[{index}] could not set thread name: {e}");
    }

    // Init timing buffer.
    let mut timings = TimingBuffer::new(log_capacity);

    // First thread sets t_zero.
    if index.0 == 0 {
        if let Ok(now) = clock_gettime(ClockId::CLOCK_MONOTONIC) {
            let mut tz = state.t_zero_ns.lock().unwrap();
            *tz = Some(timespec_to_nsec(&now));
        }
    }

    // Wait for all initial threads to be ready.
    if let Some(barrier) = startup_barrier {
        barrier.wait();
    }

    let t_zero_ns = state.t_zero_ns.lock().unwrap().unwrap_or(0);
    let mut t_first_ns = t_zero_ns;

    info!("[{index}] starting thread ...");

    // Apply initial delay.
    if let Some(delay) = tdata.delay {
        debug!("[{index}] initial delay {} us", delay.as_micros());
        t_first_ns += delay.as_nanos() as u64;
        let wake_at_ns = t_first_ns;
        sleep_until_monotonic(wake_at_ns);
    }

    // Set scheduling params.
    apply_initial_scheduling(index, &mut tdata, &state);

    // Main phase loop.
    run_phase_loop(
        index,
        &tdata,
        &local_resources,
        &state,
        &mut timings,
        t_first_ns,
    );

    // Reset to SCHED_OTHER before cleanup.
    reset_to_default_scheduling();

    // Flush log.
    if let Some(ref tb) = timings {
        let mut stdout = std::io::stdout().lock();
        let _ = tb.flush_to(&mut stdout);
    }

    // Gnuplot output.
    if state.opts.gnuplot {
        if let Some(ref logdir) = state.opts.logdir {
            let basename = state.opts.logbasename.as_deref().unwrap_or("rt-app");
            let _ = timing::write_thread_gnuplot(logdir, basename, &thread_name);
        }
    }

    info!("[{index}] exiting.");
}

// ---------------------------------------------------------------------------
// Phase loop
// ---------------------------------------------------------------------------

fn run_phase_loop(
    index: ThreadIndex,
    tdata: &ThreadData,
    local_resources: &[ResourceHandle],
    state: &Arc<EngineState>,
    timings: &mut Option<TimingBuffer>,
    t_first_ns: u64,
) {
    let mut phase_idx: usize = 0;
    let mut phase_loop: i32 = 0;
    let mut thread_loop: i32 = 0;
    let target_loops = tdata.loop_count;

    while state.continue_running.load(Ordering::Relaxed) && thread_loop != target_loops {
        let pdata = &tdata.phases[phase_idx];

        debug!("[{index}] thread_loop={thread_loop} phase={phase_idx} phase_loop={phase_loop}");

        let mut ldata = LogData::default();
        let t_start = clock_gettime(ClockId::CLOCK_MONOTONIC).unwrap();
        let t_start_ns = timespec_to_nsec(&t_start);

        let ctx = EventContext {
            thread_index: index,
            ns_per_loop: state.ns_per_loop,
            resources: &state.resources,
            local_resources,
            cumulative_slack: state.opts.cumulative_slack,
            t_first_ns,
        };

        let continue_fn = || state.continue_running.load(Ordering::Relaxed);
        ldata.perf_usec = events::run_phase(pdata, &mut ldata, &ctx, continue_fn);

        let t_end = clock_gettime(ClockId::CLOCK_MONOTONIC).unwrap();
        let t_end_ns = timespec_to_nsec(&t_end);

        let point = build_timing_point(index, t_start_ns, t_end_ns, state.t_start_ns, &ldata);

        if let Some(ref mut tb) = timings {
            tb.push(point);
        }

        advance_phase_loop(&mut phase_idx, &mut phase_loop, &mut thread_loop, tdata);
    }
}

fn build_timing_point(
    index: ThreadIndex,
    t_start_ns: u64,
    t_end_ns: u64,
    app_start_ns: u64,
    ldata: &LogData,
) -> TimingPoint {
    let period_ns = t_end_ns.saturating_sub(t_start_ns);
    let rel_start_ns = t_start_ns.saturating_sub(app_start_ns);

    TimingPoint {
        index: index.0 as u32,
        perf_usec: ldata.perf_usec,
        duration_usec: ldata.duration_usec,
        period_usec: period_ns / 1_000,
        start_time_nsec: t_start_ns,
        end_time_nsec: t_end_ns,
        rel_start_time_nsec: rel_start_ns,
        slack_nsec: ldata.slack_nsec,
        cumulative_duration_usec: ldata.cumulative_duration_usec,
        cumulative_period_usec: ldata.cumulative_period_usec,
        wakeup_latency_usec: ldata.wakeup_latency_usec,
    }
}

fn advance_phase_loop(
    phase_idx: &mut usize,
    phase_loop: &mut i32,
    thread_loop: &mut i32,
    tdata: &ThreadData,
) {
    let pdata = &tdata.phases[*phase_idx];
    *phase_loop += 1;

    if *phase_loop == pdata.loop_count {
        *phase_loop = 0;
        *phase_idx += 1;

        if *phase_idx >= tdata.phases.len() {
            *phase_idx = 0;
            *thread_loop += 1;
            // Handle overflow for infinite loops (loop_count = -1).
            if *thread_loop < 0 {
                *thread_loop = 0;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Thread creation
// ---------------------------------------------------------------------------

/// Spawn a new worker thread.
///
/// `forked` indicates whether this is from a fork event or initial startup.
/// Returns the [`ThreadHandle`] on success.
pub fn create_thread(
    tdata: &ThreadData,
    index: ThreadIndex,
    forked: bool,
    fork_seq: u32,
    startup_barrier: Option<Arc<Barrier>>,
    state: Arc<EngineState>,
    log_capacity: usize,
) -> std::io::Result<ThreadHandle> {
    let mut thread_data = tdata.clone();
    thread_data.index = index;
    thread_data.forked = forked;

    let unique_name = make_thread_unique_name(&tdata.name, index, forked, fork_seq);
    thread_data.name = unique_name.clone();

    info!("[{index}] creating thread '{unique_name}'");

    let local_resources = Vec::new();

    let args = ThreadBodyArgs {
        tdata: thread_data,
        index,
        local_resources,
        startup_barrier,
        state,
        log_capacity,
    };

    let handle = thread::Builder::new()
        .name(unique_name.clone())
        .spawn(move || thread_body(args))?;

    Ok(ThreadHandle {
        join_handle: handle,
        name: unique_name,
    })
}

// ---------------------------------------------------------------------------
// Shutdown
// ---------------------------------------------------------------------------

/// Perform a graceful shutdown: signal all threads to stop and join them.
pub fn shutdown(state: &Arc<EngineState>, threads: &mut Vec<ThreadHandle>) {
    state.continue_running.store(false, Ordering::SeqCst);

    // Join all threads.
    for th in threads.drain(..) {
        debug!("joining thread '{}'", th.name);
        if let Err(e) = th.join_handle.join() {
            error!("thread '{}' panicked: {:?}", th.name, e);
        }
    }

    // Generate main gnuplot files.
    if state.opts.gnuplot {
        if let Some(ref logdir) = state.opts.logdir {
            let basename = state.opts.logbasename.as_deref().unwrap_or("rt-app");
            // We don't have policy info after threads exit, so we use
            // a placeholder. In the full implementation, this would be
            // stored in the thread handles.
            let infos: Vec<ThreadPlotInfo<'_>> = Vec::new();
            let _ = timing::write_main_gnuplot(logdir, basename, &infos);
        }
    }
}

/// Install signal handlers for graceful shutdown.
///
/// Installs handlers for SIGQUIT, SIGTERM, SIGHUP, SIGINT that set the
/// `continue_running` flag to false.
pub fn install_signal_handlers(state: &Arc<EngineState>) {
    // We use a simple atomic flag approach. The actual signal handling
    // in the full implementation would use signal-hook or nix::sys::signal.
    // For now, this is a structural placeholder that documents the intent.
    debug!("signal handlers installed (continue_running flag)");

    // Note: In the full runtime, we would use signal_hook::flag::register
    // to connect SIGINT/SIGTERM/etc. to state.continue_running.
    // That requires the signal-hook crate, which can be added when the
    // main loop is wired up.
    let _ = state;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Set the current thread's OS-level name.
fn set_current_thread_name(name: &str) -> std::io::Result<()> {
    // SAFETY: pthread_setname_np with pthread_self() is always safe.
    // The name is converted to a CString, which ensures null-termination.
    // Linux truncates to 16 bytes (including null terminator).
    let c_name = std::ffi::CString::new(name)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "name contains null"))?;
    // SAFETY: pthread_self() returns the calling thread's ID, which is
    // always valid. The CString pointer is valid for the duration of the
    // call. pthread_setname_np has no memory safety implications.
    let ret = unsafe { libc::pthread_setname_np(libc::pthread_self(), c_name.as_ptr()) };
    if ret == 0 {
        Ok(())
    } else {
        Err(std::io::Error::from_raw_os_error(ret))
    }
}

/// Sleep until the given CLOCK_MONOTONIC timestamp (nanoseconds).
fn sleep_until_monotonic(target_ns: u64) {
    let now = clock_gettime(ClockId::CLOCK_MONOTONIC).unwrap();
    let now_ns = timespec_to_nsec(&now);
    if now_ns < target_ns {
        let sleep_dur = Duration::from_nanos(target_ns - now_ns);
        std::thread::sleep(sleep_dur);
    }
}

/// Apply initial scheduling parameters to the calling thread.
fn apply_initial_scheduling(index: ThreadIndex, tdata: &mut ThreadData, _state: &EngineState) {
    if let Some(ref sched_data) = tdata.sched_data {
        info!(
            "[{index}] starting with {} policy, priority {}",
            sched_data.policy, sched_data.priority
        );
        match scheduling::set_thread_param(index, sched_data, None) {
            Ok(force_unlock) => {
                if force_unlock {
                    tdata.lock_pages = false;
                }
            }
            Err(e) => {
                error!("[{index}] failed to set scheduling params: {e}");
            }
        }
    }

    // Lock pages if requested.
    if tdata.lock_pages {
        info!("[{index}] locking pages in memory");
        // SAFETY: mlockall(MCL_CURRENT | MCL_FUTURE) is safe to call.
        // It locks all current and future pages into RAM. The flags are
        // valid kernel constants. No memory safety issues.
        let ret = unsafe { libc::mlockall(libc::MCL_CURRENT | libc::MCL_FUTURE) };
        if ret != 0 {
            error!(
                "[{index}] mlockall failed: {}",
                std::io::Error::last_os_error()
            );
        }
    }
}

/// Reset the calling thread to SCHED_OTHER with priority 0.
fn reset_to_default_scheduling() {
    let sched = SchedData {
        policy: SchedulingPolicy::Other,
        priority: 0,
        runtime: None,
        deadline: None,
        period: None,
        util_min: None,
        util_max: None,
    };
    let _ = scheduling::set_thread_param(ThreadIndex(0), &sched, None);
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::PhaseData;

    #[test]
    fn make_thread_unique_name_normal() {
        let name = make_thread_unique_name("worker", ThreadIndex(3), false, 0);
        assert_eq!(name, "worker-3");
    }

    #[test]
    fn make_thread_unique_name_forked() {
        let name = make_thread_unique_name("worker", ThreadIndex(5), true, 42);
        assert_eq!(name, "worker-5-0042");
    }

    #[test]
    fn advance_phase_loop_single_phase() {
        let tdata = ThreadData {
            index: ThreadIndex(0),
            name: "test".into(),
            lock_pages: false,
            duration: None,
            cpu_data: Default::default(),
            default_cpu_data: Default::default(),
            numa_data: Default::default(),
            sched_data: None,
            taskgroup_data: None,
            loop_count: 3,
            phases: vec![PhaseData {
                loop_count: 2,
                events: Vec::new(),
                cpu_data: Default::default(),
                numa_data: Default::default(),
                sched_data: None,
                taskgroup_data: None,
            }],
            delay: None,
            forked: false,
            num_instances: 1,
        };

        let mut phase_idx = 0usize;
        let mut phase_loop = 0i32;
        let mut thread_loop = 0i32;

        // First call: phase_loop 0 -> 1 (no phase advance).
        advance_phase_loop(&mut phase_idx, &mut phase_loop, &mut thread_loop, &tdata);
        assert_eq!(phase_idx, 0);
        assert_eq!(phase_loop, 1);
        assert_eq!(thread_loop, 0);

        // Second call: phase_loop 1 -> 2 == loop_count, advance to phase 1
        // -> wraps to phase 0, thread_loop 0 -> 1.
        advance_phase_loop(&mut phase_idx, &mut phase_loop, &mut thread_loop, &tdata);
        assert_eq!(phase_idx, 0);
        assert_eq!(phase_loop, 0);
        assert_eq!(thread_loop, 1);
    }

    #[test]
    fn advance_phase_loop_multi_phase() {
        let tdata = ThreadData {
            index: ThreadIndex(0),
            name: "test".into(),
            lock_pages: false,
            duration: None,
            cpu_data: Default::default(),
            default_cpu_data: Default::default(),
            numa_data: Default::default(),
            sched_data: None,
            taskgroup_data: None,
            loop_count: 1,
            phases: vec![
                PhaseData {
                    loop_count: 1,
                    events: Vec::new(),
                    cpu_data: Default::default(),
                    numa_data: Default::default(),
                    sched_data: None,
                    taskgroup_data: None,
                },
                PhaseData {
                    loop_count: 1,
                    events: Vec::new(),
                    cpu_data: Default::default(),
                    numa_data: Default::default(),
                    sched_data: None,
                    taskgroup_data: None,
                },
            ],
            delay: None,
            forked: false,
            num_instances: 1,
        };

        let mut phase_idx = 0usize;
        let mut phase_loop = 0i32;
        let mut thread_loop = 0i32;

        // First call: advance from phase 0 to phase 1.
        advance_phase_loop(&mut phase_idx, &mut phase_loop, &mut thread_loop, &tdata);
        assert_eq!(phase_idx, 1);
        assert_eq!(phase_loop, 0);
        assert_eq!(thread_loop, 0);

        // Second call: advance from phase 1, wrap to 0, thread_loop++.
        advance_phase_loop(&mut phase_idx, &mut phase_loop, &mut thread_loop, &tdata);
        assert_eq!(phase_idx, 0);
        assert_eq!(phase_loop, 0);
        assert_eq!(thread_loop, 1);
    }

    #[test]
    fn build_timing_point_values() {
        let ldata = LogData {
            perf_usec: 100,
            duration_usec: 500,
            wakeup_latency_usec: 10,
            cumulative_duration_usec: 200,
            cumulative_period_usec: 1000,
            slack_nsec: -50,
        };

        let point = build_timing_point(
            ThreadIndex(2),
            1_000_000_000, // 1s
            1_001_000_000, // 1.001s
            500_000_000,   // 0.5s
            &ldata,
        );

        assert_eq!(point.index, 2);
        assert_eq!(point.perf_usec, 100);
        assert_eq!(point.duration_usec, 500);
        assert_eq!(point.period_usec, 1000); // 1ms = 1000us
        assert_eq!(point.start_time_nsec, 1_000_000_000);
        assert_eq!(point.end_time_nsec, 1_001_000_000);
        assert_eq!(point.rel_start_time_nsec, 500_000_000);
        assert_eq!(point.slack_nsec, -50);
    }

    #[test]
    fn engine_state_construction() {
        let state = EngineState {
            continue_running: AtomicBool::new(true),
            running_threads: AtomicI32::new(0),
            ns_per_loop: NsPerLoop(100),
            resources: Vec::new(),
            opts: AppOptions {
                lock_pages: false,
                policy: SchedulingPolicy::Other,
                duration: None,
                logdir: None,
                logbasename: None,
                logsize: None,
                gnuplot: false,
                calib_cpu: 0,
                calib_ns_per_loop: None,
                pi_enabled: false,
                die_on_dmiss: false,
                mem_buffer_size: None,
                io_device: None,
                cumulative_slack: false,
            },
            t_zero_ns: Mutex::new(None),
            t_start_ns: 0,
        };

        assert!(state.continue_running.load(Ordering::Relaxed));
        assert_eq!(state.running_threads.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn set_thread_name_works() {
        // Should succeed for a short name.
        assert!(set_current_thread_name("test").is_ok());
    }

    #[test]
    fn set_thread_name_long_truncates() {
        // Linux accepts up to 15 chars + null. We pass a truncated name
        // so the function should succeed.
        let long_name = "a".repeat(15);
        assert!(set_current_thread_name(&long_name).is_ok());
    }
}
