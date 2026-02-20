//! CPU calibration routines for rt-app-rs.
//!
//! Ported from the calibration portions of `rt-app.c`.
//!
//! The calibration determines the cost of one iteration of the
//! [`waste_cpu_cycles`] busy loop, expressed as nanoseconds per loop.
//! Two methods are used and the minimum (highest capacity) is returned.

use std::time::Duration;

use log::debug;
use nix::time::{clock_gettime, ClockId};

use crate::utils::timespec_to_nsec;

// ---------------------------------------------------------------------------
// NsPerLoop — strong type for the calibration result
// ---------------------------------------------------------------------------

/// Nanoseconds per busy-loop iteration. The core calibration output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct NsPerLoop(pub u32);

impl NsPerLoop {
    /// Convert a requested execution time (in microseconds) to a loop count.
    pub fn exec_to_loops(self, exec_usec: u64) -> u64 {
        if self.0 == 0 {
            return 0;
        }
        (exec_usec * 1_000) / u64::from(self.0)
    }
}

// ---------------------------------------------------------------------------
// ldexp — pure Rust equivalent of C's ldexp(x, n)
// ---------------------------------------------------------------------------

/// Compute `x * 2^n`, equivalent to C's `ldexp(x, n)`.
#[inline]
fn ldexp(x: f64, n: f64) -> f64 {
    x * (n as i32 as f64).exp2()
}

// ---------------------------------------------------------------------------
// waste_cpu_cycles — the core busy loop
// ---------------------------------------------------------------------------

/// Burn CPU cycles by performing floating-point `ldexp`-like operations.
///
/// This deliberately wastes `load_loops` iterations of CPU time. The
/// operations are chosen to be heavy enough that the compiler cannot
/// trivially optimise them away, while remaining deterministic.
///
/// Corresponds to `waste_cpu_cycles()` in the C original.
#[inline(never)]
pub fn waste_cpu_cycles(load_loops: u64) {
    let param: f64 = 0.95;
    let n: f64 = 4.0;
    for _ in 0..load_loops {
        // Four rounds of nested ldexp, matching the C original.
        // `std::hint::black_box` prevents the compiler from eliminating
        // the dead computation.
        std::hint::black_box(ldexp(param, ldexp(param, ldexp(param, n))));
        std::hint::black_box(ldexp(param, ldexp(param, ldexp(param, n))));
        std::hint::black_box(ldexp(param, ldexp(param, ldexp(param, n))));
        std::hint::black_box(ldexp(param, ldexp(param, ldexp(param, n))));
    }
}

// ---------------------------------------------------------------------------
// Calibration helpers — shared convergence logic
// ---------------------------------------------------------------------------

/// Maximum number of calibration trials before giving up.
const CAL_TRIALS: u32 = 1000;

/// Starting loop count for calibration.
const INITIAL_LOAD_LOOPS: u64 = 10_000;

/// Amount to add to the loop count each trial for randomisation.
const LOOP_INCREMENT: u64 = 33_333;

/// Modulus for loop-count randomisation.
const LOOP_MODULUS: u64 = 1_000_000;

/// Convergence threshold: `|sample - avg| * 50 < avg`.
fn converged(sample: u32, avg: u32) -> bool {
    let diff = (i64::from(sample) - i64::from(avg)).unsigned_abs();
    diff * 50 < u64::from(avg)
}

/// Shared calibration loop body. The `pre_measurement` callback is called
/// before each measurement burst (used by method 1 to inject a sleep).
fn calibrate_loop(clock: ClockId, mut pre_measurement: impl FnMut()) -> Option<NsPerLoop> {
    let mut max_load_loop = INITIAL_LOAD_LOOPS;
    let mut avg_per_loop: u32 = 0;

    for _ in 0..CAL_TRIALS {
        pre_measurement();

        let start = clock_gettime(clock).ok()?;
        waste_cpu_cycles(max_load_loop);
        let stop = clock_gettime(clock).ok()?;

        let diff_ns = timespec_to_nsec(&stop).wrapping_sub(timespec_to_nsec(&start));
        let nsec_per_loop = (diff_ns / max_load_loop) as u32;
        avg_per_loop = (avg_per_loop + nsec_per_loop) >> 1;

        if converged(nsec_per_loop, avg_per_loop) {
            return Some(NsPerLoop(avg_per_loop));
        }

        max_load_loop += LOOP_INCREMENT;
        max_load_loop %= LOOP_MODULUS;
        // Avoid zero -- keep at least the increment.
        if max_load_loop == 0 {
            max_load_loop = LOOP_INCREMENT;
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Public calibration API
// ---------------------------------------------------------------------------

/// Calibration method 1: alternates idle periods (1-second sleep) with
/// measured bursts to avoid triggering thermal mitigation.
pub fn calibrate_cpu_cycles_1(clock: ClockId) -> Option<NsPerLoop> {
    debug!("calibrate_cpu_cycles_1: starting (with idle gaps)");
    calibrate_loop(clock, || {
        std::thread::sleep(Duration::from_secs(1));
    })
}

/// Calibration method 2: continuous bursts to push the CPU governor to
/// maximum frequency.
pub fn calibrate_cpu_cycles_2(clock: ClockId) -> Option<NsPerLoop> {
    debug!("calibrate_cpu_cycles_2: starting (continuous)");
    calibrate_loop(clock, || {})
}

/// Run both calibration methods and return the minimum (corresponding to
/// the highest achievable compute capacity).
///
/// Returns `None` only if both methods fail to converge within their
/// trial limits.
pub fn calibrate_cpu_cycles(clock: ClockId) -> Option<NsPerLoop> {
    let calib1 = calibrate_cpu_cycles_1(clock);
    let calib2 = calibrate_cpu_cycles_2(clock);

    match (calib1, calib2) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) | (None, Some(a)) => Some(a),
        (None, None) => None,
    }
}

// ---------------------------------------------------------------------------
// loadwait -- convert execution time to loop count and burn
// ---------------------------------------------------------------------------

/// Execute CPU-busy work for the specified duration (microseconds).
///
/// For large durations, the work is split into 1-second bursts to avoid
/// overflowing the loop counter. Returns the "performance" metric, which
/// is `exec_usec / ns_per_loop` (the fixed amount of work performed).
pub fn loadwait(exec_usec: u64, ns_per_loop: NsPerLoop) -> u64 {
    if ns_per_loop.0 == 0 {
        return 0;
    }
    let p_load = u64::from(ns_per_loop.0);
    let perf = exec_usec / p_load;

    let mut remaining = exec_usec;
    let secs = remaining / 1_000_000;

    for _ in 0..secs {
        let load_count = 1_000_000_000 / p_load;
        waste_cpu_cycles(load_count);
        remaining -= 1_000_000;
    }

    // Remaining fraction.
    let load_count = (remaining * 1_000) / p_load;
    waste_cpu_cycles(load_count);

    perf
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ns_per_loop_ordering() {
        let a = NsPerLoop(10);
        let b = NsPerLoop(20);
        assert!(a < b);
        assert_eq!(a.min(b), a);
    }

    #[test]
    fn ns_per_loop_exec_to_loops() {
        let ns = NsPerLoop(100);
        // 1000 usec = 1_000_000 ns => 1_000_000 / 100 = 10_000 loops
        assert_eq!(ns.exec_to_loops(1000), 10_000);
    }

    #[test]
    fn ns_per_loop_exec_to_loops_zero() {
        let ns = NsPerLoop(0);
        assert_eq!(ns.exec_to_loops(1000), 0);
    }

    #[test]
    fn convergence_check() {
        assert!(converged(100, 100));
        assert!(!converged(100, 200));
        assert!(converged(100, 101));
    }

    #[test]
    fn convergence_zero_avg_not_converged() {
        // avg=0 => threshold is 0, so diff*50 < 0 is always false.
        assert!(!converged(1, 0));
        // 0 vs 0: diff=0, 0*50=0 < 0 is false => not converged.
        // This matches the C code behaviour where avg=0 never converges.
        assert!(!converged(0, 0));
    }

    #[test]
    fn waste_cpu_cycles_runs_without_panic() {
        waste_cpu_cycles(100);
    }

    #[test]
    fn ldexp_basic() {
        // ldexp(1.0, 3) = 1.0 * 2^3 = 8.0
        assert!((ldexp(1.0, 3.0) - 8.0).abs() < f64::EPSILON);
        // ldexp(0.5, 1) = 0.5 * 2^1 = 1.0
        assert!((ldexp(0.5, 1.0) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn ldexp_matches_c_pattern() {
        // The C code does: ldexp(0.95, ldexp(0.95, ldexp(0.95, 4)))
        // Inner: ldexp(0.95, 4) = 0.95 * 2^4 = 0.95 * 16 = 15.2
        // Middle: ldexp(0.95, 15) = 0.95 * 2^15 = 0.95 * 32768 = 31129.6
        // (n is truncated to i32 so 15.2 -> 15)
        let inner = ldexp(0.95, 4.0);
        assert!((inner - 15.2).abs() < 0.001);
    }

    #[test]
    fn loadwait_zero_ns_per_loop() {
        assert_eq!(loadwait(1000, NsPerLoop(0)), 0);
    }

    #[test]
    fn loadwait_small_duration() {
        let perf = loadwait(500, NsPerLoop(100));
        assert_eq!(perf, 5);
    }

    #[test]
    fn loadwait_large_duration_splits() {
        let perf = loadwait(2_500_000, NsPerLoop(10));
        assert_eq!(perf, 250_000);
    }
}
