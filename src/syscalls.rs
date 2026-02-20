//! SCHED_DEADLINE syscall wrappers for Linux.
//!
//! Ported from the C implementation in `libdl/dl_syscalls.c` and
//! `libdl/dl_syscalls.h`. Provides the `sched_setattr()` and
//! `sched_getattr()` system calls which are not exposed by glibc
//! and must be invoked via `syscall(2)`.

use std::io;
use std::mem;

use bitflags::bitflags;

/// Linux scheduling policy constant for SCHED_DEADLINE.
pub const SCHED_DEADLINE: u32 = 6;

/// The expected size of [`SchedAttr`] as defined by the Linux kernel ABI.
/// This must match `sizeof(struct sched_attr)` in the kernel headers.
pub const SCHED_ATTR_SIZE: u32 = 56;

/// Linux `struct sched_attr` for use with `sched_setattr(2)` / `sched_getattr(2)`.
///
/// Field layout matches the kernel definition exactly. All fields use
/// fixed-width integers matching the kernel's `__u32`, `__u64`, `__s32` types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct SchedAttr {
    /// Size of this structure (must be set to `SCHED_ATTR_SIZE`).
    pub size: u32,

    /// Scheduling policy (SCHED_NORMAL, SCHED_FIFO, SCHED_RR, SCHED_DEADLINE, etc.).
    pub sched_policy: u32,

    /// Scheduling flags (see [`SchedFlags`]).
    pub sched_flags: u64,

    /// Nice value for SCHED_NORMAL / SCHED_BATCH.
    pub sched_nice: i32,

    /// Static priority for SCHED_FIFO / SCHED_RR.
    pub sched_priority: u32,

    /// SCHED_DEADLINE runtime parameter (nanoseconds).
    pub sched_runtime: u64,

    /// SCHED_DEADLINE deadline parameter (nanoseconds).
    pub sched_deadline: u64,

    /// SCHED_DEADLINE period parameter (nanoseconds).
    pub sched_period: u64,

    /// Utilization hint: minimum (for SCHED_FLAG_UTIL_CLAMP_MIN).
    pub sched_util_min: u32,

    /// Utilization hint: maximum (for SCHED_FLAG_UTIL_CLAMP_MAX).
    pub sched_util_max: u32,
}

impl Default for SchedAttr {
    fn default() -> Self {
        Self {
            size: SCHED_ATTR_SIZE,
            sched_policy: 0,
            sched_flags: 0,
            sched_nice: 0,
            sched_priority: 0,
            sched_runtime: 0,
            sched_deadline: 0,
            sched_period: 0,
            sched_util_min: 0,
            sched_util_max: 0,
        }
    }
}

bitflags! {
    /// Flags for `sched_setattr(2)` / `sched_getattr(2)`, matching the
    /// `SCHED_FLAG_*` constants from the Linux kernel headers.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct SchedFlags: u64 {
        const RESET_ON_FORK   = 0x01;
        const RECLAIM         = 0x02;
        const DL_OVERRUN      = 0x04;
        const KEEP_POLICY     = 0x08;
        const KEEP_PARAMS     = 0x10;
        const UTIL_CLAMP_MIN  = 0x20;
        const UTIL_CLAMP_MAX  = 0x40;

        /// Composite: keep both policy and params.
        const KEEP_ALL        = Self::KEEP_POLICY.bits() | Self::KEEP_PARAMS.bits();
        /// Composite: both util clamp flags.
        const UTIL_CLAMP      = Self::UTIL_CLAMP_MIN.bits() | Self::UTIL_CLAMP_MAX.bits();
        /// Composite: all flags combined.
        const ALL             = Self::RESET_ON_FORK.bits()
                              | Self::RECLAIM.bits()
                              | Self::DL_OVERRUN.bits()
                              | Self::KEEP_ALL.bits()
                              | Self::UTIL_CLAMP.bits();
    }
}

/// Set scheduling attributes for a process via `sched_setattr(2)`.
///
/// # Arguments
/// * `pid` - Process ID (0 for the calling thread).
/// * `attr` - Scheduling attributes to apply.
/// * `flags` - Currently unused by the kernel; pass 0.
///
/// # Safety
/// The caller must ensure that `attr` points to a valid, properly initialized
/// `SchedAttr` struct whose `size` field equals `SCHED_ATTR_SIZE`. The `pid`
/// must refer to an existing process/thread or be 0 for the current thread.
/// This is a direct syscall wrapper with no additional validation.
///
/// # Errors
/// Returns `io::Error` if the syscall fails (e.g., EPERM, EINVAL, ESRCH).
pub unsafe fn sched_setattr(pid: libc::pid_t, attr: &SchedAttr, flags: u32) -> io::Result<()> {
    // SAFETY: We pass a valid pointer to a repr(C) struct matching the kernel
    // ABI. The caller is responsible for ensuring the pid and attr are valid.
    let ret = unsafe {
        libc::syscall(
            libc::SYS_sched_setattr,
            pid as libc::c_long,
            attr as *const SchedAttr as libc::c_long,
            flags as libc::c_long,
        )
    };
    if ret == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Get scheduling attributes for a process via `sched_getattr(2)`.
///
/// # Arguments
/// * `pid` - Process ID (0 for the calling thread).
/// * `attr` - Buffer to receive the scheduling attributes.
/// * `flags` - Currently unused by the kernel; pass 0.
///
/// # Safety
/// The caller must ensure that `attr` points to a writable `SchedAttr` buffer
/// large enough to hold the kernel's response. The `pid` must refer to an
/// existing process/thread or be 0 for the current thread. This is a direct
/// syscall wrapper with no additional validation.
///
/// # Errors
/// Returns `io::Error` if the syscall fails (e.g., EPERM, EINVAL, ESRCH).
pub unsafe fn sched_getattr(pid: libc::pid_t, attr: &mut SchedAttr, flags: u32) -> io::Result<()> {
    let size = mem::size_of::<SchedAttr>() as u32;
    // SAFETY: We pass a valid mutable pointer to a repr(C) struct matching the
    // kernel ABI, along with its correct size. The caller is responsible for
    // ensuring the pid is valid.
    let ret = unsafe {
        libc::syscall(
            libc::SYS_sched_getattr,
            pid as libc::c_long,
            attr as *mut SchedAttr as libc::c_long,
            size as libc::c_long,
            flags as libc::c_long,
        )
    };
    if ret == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sched_attr_size_matches_kernel() {
        // The Linux kernel defines struct sched_attr as 56 bytes.
        assert_eq!(
            mem::size_of::<SchedAttr>(),
            SCHED_ATTR_SIZE as usize,
            "SchedAttr size must match the kernel's struct sched_attr (56 bytes)"
        );
    }

    #[test]
    fn sched_attr_default_has_correct_size() {
        let attr = SchedAttr::default();
        assert_eq!(attr.size, SCHED_ATTR_SIZE);
        assert_eq!(attr.sched_policy, 0);
        assert_eq!(attr.sched_flags, 0);
        assert_eq!(attr.sched_nice, 0);
        assert_eq!(attr.sched_priority, 0);
        assert_eq!(attr.sched_runtime, 0);
        assert_eq!(attr.sched_deadline, 0);
        assert_eq!(attr.sched_period, 0);
        assert_eq!(attr.sched_util_min, 0);
        assert_eq!(attr.sched_util_max, 0);
    }

    #[test]
    fn sched_flags_individual_bits() {
        assert_eq!(SchedFlags::RESET_ON_FORK.bits(), 0x01);
        assert_eq!(SchedFlags::RECLAIM.bits(), 0x02);
        assert_eq!(SchedFlags::DL_OVERRUN.bits(), 0x04);
        assert_eq!(SchedFlags::KEEP_POLICY.bits(), 0x08);
        assert_eq!(SchedFlags::KEEP_PARAMS.bits(), 0x10);
        assert_eq!(SchedFlags::UTIL_CLAMP_MIN.bits(), 0x20);
        assert_eq!(SchedFlags::UTIL_CLAMP_MAX.bits(), 0x40);
    }

    #[test]
    fn sched_flags_composites() {
        assert_eq!(
            SchedFlags::KEEP_ALL,
            SchedFlags::KEEP_POLICY | SchedFlags::KEEP_PARAMS
        );
        assert_eq!(
            SchedFlags::UTIL_CLAMP,
            SchedFlags::UTIL_CLAMP_MIN | SchedFlags::UTIL_CLAMP_MAX
        );

        let expected_all = SchedFlags::RESET_ON_FORK
            | SchedFlags::RECLAIM
            | SchedFlags::DL_OVERRUN
            | SchedFlags::KEEP_ALL
            | SchedFlags::UTIL_CLAMP;
        assert_eq!(SchedFlags::ALL, expected_all);
    }

    #[test]
    fn sched_flags_bitwise_operations() {
        let flags = SchedFlags::RECLAIM | SchedFlags::DL_OVERRUN;
        assert!(flags.contains(SchedFlags::RECLAIM));
        assert!(flags.contains(SchedFlags::DL_OVERRUN));
        assert!(!flags.contains(SchedFlags::RESET_ON_FORK));
    }

    #[test]
    fn sched_flags_empty_and_all() {
        assert!(SchedFlags::empty().is_empty());
        assert!(!SchedFlags::ALL.is_empty());
        assert_eq!(SchedFlags::ALL.bits(), 0x7F);
    }

    #[test]
    fn sched_deadline_constant() {
        assert_eq!(SCHED_DEADLINE, 6);
    }

    #[test]
    fn sched_getattr_current_thread() {
        // sched_getattr with pid=0 should succeed for the current thread.
        let mut attr = SchedAttr::default();
        let result = unsafe { sched_getattr(0, &mut attr, 0) };
        assert!(
            result.is_ok(),
            "sched_getattr on current thread should succeed: {:?}",
            result.err()
        );
        // The kernel fills in the size field.
        assert!(attr.size > 0);
    }
}
