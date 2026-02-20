//! Scheduling parameter setup for rt-app threads.
//!
//! Ported from `set_thread_param()` and the `_set_thread_{cfs,rt,deadline,uclamp}`
//! helpers in `rt-app.c`.

use log::{debug, error};
use thiserror::Error;

use crate::syscalls::{self, SchedAttr, SchedFlags};
use crate::types::{SchedData, SchedulingPolicy, ThreadIndex, THREAD_PRIORITY_UNCHANGED};
use crate::utils::gettid;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors arising from scheduling parameter setup.
#[derive(Debug, Error)]
pub enum SchedError {
    #[error("[{thread}] failed to set scheduling parameters: {source}")]
    SetAttr {
        thread: ThreadIndex,
        #[source]
        source: std::io::Error,
    },

    #[error("[{thread}] invalid nice value {nice}: must be -20..=19")]
    InvalidNice { thread: ThreadIndex, nice: i32 },

    #[error("[{thread}] unknown scheduling policy: {policy}")]
    UnknownPolicy {
        thread: ThreadIndex,
        policy: SchedulingPolicy,
    },
}

pub type Result<T> = std::result::Result<T, SchedError>;

// ---------------------------------------------------------------------------
// Priority helper
// ---------------------------------------------------------------------------

/// Return the `sched_priority` field value for the given policy.
///
/// `sched_priority` is only meaningful for RT policies (FIFO, RR).
/// For all others it must be 0 for `sched_setattr` to succeed.
pub fn sched_priority_for(policy: SchedulingPolicy, prio: i32) -> u32 {
    match policy {
        SchedulingPolicy::Fifo | SchedulingPolicy::RoundRobin => prio.max(0) as u32,
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// Policy-specific helpers
// ---------------------------------------------------------------------------

/// Build a [`SchedAttr`] for CFS-class policies (SCHED_OTHER, SCHED_IDLE).
pub fn build_cfs_attr(policy: SchedulingPolicy, prio: i32, runtime_ns: u64) -> SchedAttr {
    let kernel_policy = policy.to_kernel_id().unwrap_or(0);
    SchedAttr {
        sched_policy: kernel_policy,
        sched_priority: sched_priority_for(policy, prio),
        sched_nice: prio,
        sched_runtime: runtime_ns,
        ..SchedAttr::default()
    }
}

/// Build a [`SchedAttr`] for RT-class policies (SCHED_FIFO, SCHED_RR).
pub fn build_rt_attr(policy: SchedulingPolicy, prio: i32) -> SchedAttr {
    let kernel_policy = policy.to_kernel_id().unwrap_or(0);
    SchedAttr {
        sched_policy: kernel_policy,
        sched_priority: sched_priority_for(policy, prio),
        ..SchedAttr::default()
    }
}

/// Build a [`SchedAttr`] for SCHED_DEADLINE.
pub fn build_deadline_attr(runtime_ns: u64, deadline_ns: u64, period_ns: u64) -> SchedAttr {
    SchedAttr {
        sched_policy: syscalls::SCHED_DEADLINE,
        sched_priority: 0,
        sched_runtime: runtime_ns,
        sched_deadline: deadline_ns,
        sched_period: period_ns,
        ..SchedAttr::default()
    }
}

/// Build a [`SchedAttr`] for uclamp (utility clamping).
pub fn build_uclamp_attr(
    policy: SchedulingPolicy,
    prio: i32,
    util_min: Option<u32>,
    util_max: Option<u32>,
) -> SchedAttr {
    let mut flags = SchedFlags::KEEP_ALL;
    let mut attr = SchedAttr {
        sched_policy: policy.to_kernel_id().unwrap_or(0),
        sched_priority: sched_priority_for(policy, prio),
        sched_flags: flags.bits(),
        ..SchedAttr::default()
    };

    if let Some(min) = util_min {
        attr.sched_util_min = min;
        flags |= SchedFlags::UTIL_CLAMP_MIN;
    }
    if let Some(max) = util_max {
        attr.sched_util_max = max;
        flags |= SchedFlags::UTIL_CLAMP_MAX;
    }
    attr.sched_flags = flags.bits();
    attr
}

// ---------------------------------------------------------------------------
// Apply helpers
// ---------------------------------------------------------------------------

/// Apply a [`SchedAttr`] to the calling thread via `sched_setattr(2)`.
fn apply_sched_attr(thread: ThreadIndex, attr: &SchedAttr) -> Result<()> {
    let tid = gettid().as_raw();
    // SAFETY: We pass a valid, properly initialised SchedAttr with correct
    // size field. The pid is the current thread's TID (always valid).
    // sched_setattr is a direct kernel syscall with no memory safety
    // implications beyond the struct layout, which is guaranteed by repr(C).
    unsafe { syscalls::sched_setattr(tid, attr, 0) }
        .map_err(|e| SchedError::SetAttr { thread, source: e })
}

/// Set CFS scheduling parameters.
fn set_thread_cfs(
    thread: ThreadIndex,
    sched_data: &SchedData,
    prev_policy: Option<SchedulingPolicy>,
) -> Result<()> {
    if sched_data.priority == THREAD_PRIORITY_UNCHANGED {
        return Ok(());
    }

    // Only call sched_setattr when the policy changes or is SCHED_OTHER.
    let policy_changed = prev_policy.is_none_or(|p| p != sched_data.policy);

    if sched_data.policy == SchedulingPolicy::Other {
        if !(-20..=19).contains(&sched_data.priority) {
            return Err(SchedError::InvalidNice {
                thread,
                nice: sched_data.priority,
            });
        }
        let runtime_ns = sched_data.runtime.map(|d| d.as_nanos() as u64).unwrap_or(0);
        let attr = build_cfs_attr(sched_data.policy, sched_data.priority, runtime_ns);
        debug!(
            "[{thread}] setting scheduler {} nice={} runtime={}",
            sched_data.policy, sched_data.priority, runtime_ns
        );
        apply_sched_attr(thread, &attr)?;
    } else if policy_changed {
        // For SCHED_IDLE or other CFS-like, just set the policy.
        let attr = build_rt_attr(sched_data.policy, 0);
        debug!(
            "[{thread}] setting scheduler {} priority {}",
            sched_data.policy, sched_data.priority
        );
        apply_sched_attr(thread, &attr)?;
    }

    Ok(())
}

/// Set RT scheduling parameters (FIFO, RR).
fn set_thread_rt(thread: ThreadIndex, sched_data: &SchedData) -> Result<()> {
    if sched_data.priority == THREAD_PRIORITY_UNCHANGED {
        return Ok(());
    }
    let attr = build_rt_attr(sched_data.policy, sched_data.priority);
    debug!(
        "[{thread}] setting scheduler {} priority {}",
        sched_data.policy, sched_data.priority
    );
    apply_sched_attr(thread, &attr)
}

/// Set DEADLINE scheduling parameters.
fn set_thread_deadline(thread: ThreadIndex, sched_data: &SchedData) -> Result<()> {
    let runtime_ns = sched_data.runtime.map(|d| d.as_nanos() as u64).unwrap_or(0);
    let deadline_ns = sched_data
        .deadline
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let period_ns = sched_data.period.map(|d| d.as_nanos() as u64).unwrap_or(0);

    debug!(
        "[{thread}] setting scheduler {} exec={} deadline={} period={}",
        sched_data.policy, runtime_ns, deadline_ns, period_ns
    );

    let attr = build_deadline_attr(runtime_ns, deadline_ns, period_ns);
    apply_sched_attr(thread, &attr)
}

/// Set uclamp parameters.
fn set_thread_uclamp(thread: ThreadIndex, sched_data: &SchedData) -> Result<()> {
    if sched_data.util_min.is_none() && sched_data.util_max.is_none() {
        return Ok(());
    }

    if let Some(min) = sched_data.util_min {
        debug!("[{thread}] setting util_min={min}");
    }
    if let Some(max) = sched_data.util_max {
        debug!("[{thread}] setting util_max={max}");
    }

    let attr = build_uclamp_attr(
        sched_data.policy,
        sched_data.priority,
        sched_data.util_min,
        sched_data.util_max,
    );
    apply_sched_attr(thread, &attr)
}

// ---------------------------------------------------------------------------
// Main dispatcher
// ---------------------------------------------------------------------------

/// Set the scheduling parameters for the calling thread based on the
/// given [`SchedData`]. This is the main entry point.
///
/// `prev_sched` is the previously applied scheduling data, used to avoid
/// redundant syscalls.
///
/// Returns `true` if `lock_pages` should be forced off (CFS policies).
pub fn set_thread_param(
    thread: ThreadIndex,
    sched_data: &SchedData,
    prev_sched: Option<&SchedData>,
) -> Result<bool> {
    let mut force_unlock_pages = false;

    let resolved_policy = if sched_data.policy == SchedulingPolicy::Same {
        prev_sched
            .map(|p| p.policy)
            .unwrap_or(SchedulingPolicy::Other)
    } else {
        sched_data.policy
    };

    // Create a view with the resolved policy for the sub-functions.
    let effective = SchedData {
        policy: resolved_policy,
        ..sched_data.clone()
    };

    match resolved_policy {
        SchedulingPolicy::Fifo | SchedulingPolicy::RoundRobin => {
            set_thread_rt(thread, &effective)?;
            set_thread_uclamp(thread, &effective)?;
        }
        SchedulingPolicy::Other | SchedulingPolicy::Idle => {
            let prev_policy = prev_sched.map(|p| p.policy);
            set_thread_cfs(thread, &effective, prev_policy)?;
            set_thread_uclamp(thread, &effective)?;
            force_unlock_pages = true;
        }
        SchedulingPolicy::Deadline => {
            set_thread_deadline(thread, &effective)?;
        }
        SchedulingPolicy::Same => {
            // Already resolved above; this branch is unreachable.
            error!("[{thread}] unresolved SCHED_SAME policy");
            return Err(SchedError::UnknownPolicy {
                thread,
                policy: SchedulingPolicy::Same,
            });
        }
    }

    Ok(force_unlock_pages)
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn sched_priority_rt_policies() {
        assert_eq!(sched_priority_for(SchedulingPolicy::Fifo, 50), 50);
        assert_eq!(sched_priority_for(SchedulingPolicy::RoundRobin, 99), 99);
    }

    #[test]
    fn sched_priority_non_rt_is_zero() {
        assert_eq!(sched_priority_for(SchedulingPolicy::Other, 50), 0);
        assert_eq!(sched_priority_for(SchedulingPolicy::Deadline, 50), 0);
        assert_eq!(sched_priority_for(SchedulingPolicy::Idle, 50), 0);
    }

    #[test]
    fn sched_priority_negative_clamped() {
        assert_eq!(sched_priority_for(SchedulingPolicy::Fifo, -5), 0);
    }

    #[test]
    fn build_cfs_attr_fields() {
        let attr = build_cfs_attr(SchedulingPolicy::Other, -5, 100_000);
        assert_eq!(attr.sched_policy, 0); // SCHED_OTHER
        assert_eq!(attr.sched_nice, -5);
        assert_eq!(attr.sched_priority, 0);
        assert_eq!(attr.sched_runtime, 100_000);
    }

    #[test]
    fn build_rt_attr_fields() {
        let attr = build_rt_attr(SchedulingPolicy::Fifo, 50);
        assert_eq!(attr.sched_policy, 1); // SCHED_FIFO
        assert_eq!(attr.sched_priority, 50);
        assert_eq!(attr.sched_nice, 0);
    }

    #[test]
    fn build_deadline_attr_fields() {
        let attr = build_deadline_attr(1_000_000, 5_000_000, 10_000_000);
        assert_eq!(attr.sched_policy, syscalls::SCHED_DEADLINE);
        assert_eq!(attr.sched_runtime, 1_000_000);
        assert_eq!(attr.sched_deadline, 5_000_000);
        assert_eq!(attr.sched_period, 10_000_000);
        assert_eq!(attr.sched_priority, 0);
    }

    #[test]
    fn build_uclamp_attr_both() {
        let attr = build_uclamp_attr(SchedulingPolicy::Other, 0, Some(256), Some(768));
        let flags = SchedFlags::from_bits_truncate(attr.sched_flags);
        assert!(flags.contains(SchedFlags::KEEP_ALL));
        assert!(flags.contains(SchedFlags::UTIL_CLAMP_MIN));
        assert!(flags.contains(SchedFlags::UTIL_CLAMP_MAX));
        assert_eq!(attr.sched_util_min, 256);
        assert_eq!(attr.sched_util_max, 768);
    }

    #[test]
    fn build_uclamp_attr_min_only() {
        let attr = build_uclamp_attr(SchedulingPolicy::Fifo, 50, Some(128), None);
        let flags = SchedFlags::from_bits_truncate(attr.sched_flags);
        assert!(flags.contains(SchedFlags::UTIL_CLAMP_MIN));
        assert!(!flags.contains(SchedFlags::UTIL_CLAMP_MAX));
        assert_eq!(attr.sched_util_min, 128);
        assert_eq!(attr.sched_util_max, 0);
    }

    #[test]
    fn build_uclamp_attr_none() {
        let attr = build_uclamp_attr(SchedulingPolicy::Other, 0, None, None);
        let flags = SchedFlags::from_bits_truncate(attr.sched_flags);
        assert!(!flags.contains(SchedFlags::UTIL_CLAMP_MIN));
        assert!(!flags.contains(SchedFlags::UTIL_CLAMP_MAX));
    }

    #[test]
    fn set_thread_param_resolves_same_policy() {
        // We cannot actually call sched_setattr without privileges, but
        // we can verify that the policy resolution logic works by testing
        // the error case (SCHED_SAME with no previous).
        let sched = SchedData {
            policy: SchedulingPolicy::Same,
            priority: THREAD_PRIORITY_UNCHANGED,
            runtime: None,
            deadline: None,
            period: None,
            util_min: None,
            util_max: None,
        };
        // With no previous, Same resolves to Other, but priority unchanged
        // means CFS is a no-op. Should succeed.
        let result = set_thread_param(ThreadIndex(0), &sched, None);
        // This may fail due to permissions on some systems but the
        // logic path should be exercised. If EPERM, that's fine.
        match result {
            Ok(force_unlock) => assert!(force_unlock), // CFS forces unlock
            Err(SchedError::SetAttr { .. }) => {}      // EPERM is acceptable
            Err(e) => panic!("unexpected error: {e}"),
        }
    }

    #[test]
    fn deadline_sched_data_construction() {
        let sched = SchedData {
            policy: SchedulingPolicy::Deadline,
            priority: 0,
            runtime: Some(Duration::from_micros(1000)),
            deadline: Some(Duration::from_micros(5000)),
            period: Some(Duration::from_micros(10000)),
            util_min: None,
            util_max: None,
        };
        let runtime_ns = sched.runtime.unwrap().as_nanos() as u64;
        let deadline_ns = sched.deadline.unwrap().as_nanos() as u64;
        let period_ns = sched.period.unwrap().as_nanos() as u64;
        assert_eq!(runtime_ns, 1_000_000);
        assert_eq!(deadline_ns, 5_000_000);
        assert_eq!(period_ns, 10_000_000);
    }
}
