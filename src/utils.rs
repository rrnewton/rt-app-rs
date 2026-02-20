//! Utility functions for rt-app-rs, ported from rt-app_utils.c / rt-app_utils.h.
//!
//! Time conversions lean on [`std::time::Duration`] and
//! [`nix::sys::time::TimeSpec`] instead of raw integer arithmetic.
//! Enum conversions are handled by `FromStr` / `Display` impls on the types
//! themselves (in [`crate::types`]).

use std::fmt::Write as _;
use std::io::{self, Write};
use std::time::Duration;

use nix::sys::time::TimeSpec;

use crate::types::{FtraceLevel, TimingPoint};

// ---------------------------------------------------------------------------
// Time conversions
// ---------------------------------------------------------------------------

/// Convert a [`TimeSpec`] to microseconds (rounding to nearest).
pub fn timespec_to_usec(ts: &TimeSpec) -> u64 {
    let nsec = ts.tv_sec() as u64 * 1_000_000_000 + ts.tv_nsec() as u64;
    // Integer rounding: (n + 500) / 1000
    nsec.saturating_add(500) / 1_000
}

/// Convert a [`TimeSpec`] to nanoseconds.
pub fn timespec_to_nsec(ts: &TimeSpec) -> u64 {
    ts.tv_sec() as u64 * 1_000_000_000 + ts.tv_nsec() as u64
}

/// Convert microseconds to a [`TimeSpec`].
pub fn usec_to_timespec(usec: u64) -> TimeSpec {
    TimeSpec::new(
        (usec / 1_000_000) as i64,
        ((usec % 1_000_000) * 1_000) as i64,
    )
}

/// Convert milliseconds to a [`TimeSpec`].
pub fn msec_to_timespec(msec: u64) -> TimeSpec {
    TimeSpec::new((msec / 1_000) as i64, ((msec % 1_000) * 1_000_000) as i64)
}

/// Convert a [`Duration`] to a [`TimeSpec`].
pub fn duration_to_timespec(d: &Duration) -> TimeSpec {
    TimeSpec::new(d.as_secs() as i64, d.subsec_nanos() as i64)
}

/// Convert a [`TimeSpec`] to a [`Duration`].
///
/// Returns `None` if the timespec represents a negative duration.
pub fn timespec_to_duration(ts: &TimeSpec) -> Option<Duration> {
    if ts.tv_sec() < 0 {
        return None;
    }
    Some(Duration::new(ts.tv_sec() as u64, ts.tv_nsec() as u32))
}

/// Compute `t1 - t2` as a signed nanosecond value.
pub fn timespec_diff_nsec(t1: &TimeSpec, t2: &TimeSpec) -> i64 {
    let s1 = t1.tv_sec();
    let s2 = t2.tv_sec();
    let ns1 = t1.tv_nsec();
    let ns2 = t2.tv_nsec();
    (s1 - s2) * 1_000_000_000 + (ns1 - ns2)
}

// ---------------------------------------------------------------------------
// Logging
// ---------------------------------------------------------------------------

/// Write a [`TimingPoint`] as a single columnar line to the given writer.
///
/// Format matches the C original so that existing analysis scripts still work.
pub fn log_timing<W: Write>(writer: &mut W, t: &TimingPoint) -> io::Result<()> {
    writeln!(
        writer,
        "{:>4} {:>8} {:>8} {:>8} {:>15} {:>15} {:>15} {:>10} {:>10} {:>10} {:>10}",
        t.index,
        t.perf_usec,
        t.duration_usec,
        t.period_usec,
        t.start_time_nsec,
        t.end_time_nsec,
        t.rel_start_time_nsec,
        t.slack_nsec,
        t.cumulative_duration_usec,
        t.cumulative_period_usec,
        t.wakeup_latency_usec,
    )
}

// ---------------------------------------------------------------------------
// Thread ID
// ---------------------------------------------------------------------------

/// Return the kernel thread ID of the calling thread.
///
/// This wraps `gettid(2)` via the nix crate.
pub fn gettid() -> nix::unistd::Pid {
    nix::unistd::gettid()
}

// ---------------------------------------------------------------------------
// Ftrace helpers
// ---------------------------------------------------------------------------

/// Parse a comma-separated string of ftrace category names into [`FtraceLevel`].
///
/// Known category names (case-insensitive): `main`, `task`, `loop`, `event`,
/// `stats`, `attrs`, `none`.
///
/// Returns an error string if any token is unrecognised.
pub fn ftrace_parse(categories: &str) -> Result<FtraceLevel, String> {
    let mut level = FtraceLevel::empty();
    for token in categories.split(',') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        match token.to_ascii_lowercase().as_str() {
            "main" => level |= FtraceLevel::MAIN,
            "task" => level |= FtraceLevel::TASK,
            "loop" => level |= FtraceLevel::LOOP,
            "event" => level |= FtraceLevel::EVENT,
            "stats" => level |= FtraceLevel::STATS,
            "attrs" => level |= FtraceLevel::ATTRS,
            "none" => level = FtraceLevel::empty(),
            other => return Err(format!("unknown ftrace category: {other:?}")),
        }
    }
    Ok(level)
}

/// Write a formatted message to the ftrace `trace_marker` file descriptor.
///
/// This is a no-op if `marker_fd` is `None`.  The caller is responsible for
/// checking the [`FtraceLevel`] before calling this function to avoid
/// unnecessary formatting work.
pub fn ftrace_write<W: Write>(marker: &mut W, msg: &str) -> io::Result<()> {
    marker.write_all(msg.as_bytes())
}

/// Format and write an ftrace message (convenience wrapper around [`ftrace_write`]).
pub fn ftrace_write_fmt<W: Write>(marker: &mut W, args: std::fmt::Arguments<'_>) -> io::Result<()> {
    let mut buf = String::with_capacity(128);
    buf.write_fmt(args)
        .expect("formatting into String never fails");
    ftrace_write(marker, &buf)
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Time conversions --------------------------------------------------

    #[test]
    fn timespec_to_usec_basic() {
        let ts = TimeSpec::new(1, 500_000);
        assert_eq!(timespec_to_usec(&ts), 1_000_500);
    }

    #[test]
    fn timespec_to_usec_rounding() {
        // 1499 nsec -> rounds to 1 usec (1499 + 500 = 1999 / 1000 = 1)
        let ts = TimeSpec::new(0, 1_499);
        assert_eq!(timespec_to_usec(&ts), 1);
        // 500 nsec -> rounds to 1 usec (500 + 500 = 1000 / 1000 = 1)
        let ts = TimeSpec::new(0, 500);
        assert_eq!(timespec_to_usec(&ts), 1);
        // 499 nsec -> rounds to 0 usec
        let ts = TimeSpec::new(0, 499);
        assert_eq!(timespec_to_usec(&ts), 0);
    }

    #[test]
    fn timespec_to_nsec_basic() {
        let ts = TimeSpec::new(2, 123_456_789);
        assert_eq!(timespec_to_nsec(&ts), 2_123_456_789);
    }

    #[test]
    fn usec_to_timespec_basic() {
        let ts = usec_to_timespec(1_500_000);
        assert_eq!(ts.tv_sec(), 1);
        assert_eq!(ts.tv_nsec(), 500_000_000);
    }

    #[test]
    fn usec_to_timespec_zero() {
        let ts = usec_to_timespec(0);
        assert_eq!(ts.tv_sec(), 0);
        assert_eq!(ts.tv_nsec(), 0);
    }

    #[test]
    fn msec_to_timespec_basic() {
        let ts = msec_to_timespec(2500);
        assert_eq!(ts.tv_sec(), 2);
        assert_eq!(ts.tv_nsec(), 500_000_000);
    }

    #[test]
    fn duration_timespec_roundtrip() {
        let d = Duration::new(3, 141_592_653);
        let ts = duration_to_timespec(&d);
        let d2 = timespec_to_duration(&ts).unwrap();
        assert_eq!(d, d2);
    }

    #[test]
    fn timespec_to_duration_negative_returns_none() {
        let ts = TimeSpec::new(-1, 0);
        assert!(timespec_to_duration(&ts).is_none());
    }

    #[test]
    fn timespec_diff_nsec_basic() {
        let t1 = TimeSpec::new(5, 500_000_000);
        let t2 = TimeSpec::new(3, 200_000_000);
        assert_eq!(timespec_diff_nsec(&t1, &t2), 2_300_000_000);
    }

    #[test]
    fn timespec_diff_nsec_negative() {
        let t1 = TimeSpec::new(1, 0);
        let t2 = TimeSpec::new(2, 0);
        assert_eq!(timespec_diff_nsec(&t1, &t2), -1_000_000_000);
    }

    #[test]
    fn timespec_diff_nsec_borrow() {
        // t1 has smaller nsec than t2 -> tests the borrow path
        let t1 = TimeSpec::new(5, 100_000_000);
        let t2 = TimeSpec::new(3, 900_000_000);
        assert_eq!(timespec_diff_nsec(&t1, &t2), 1_200_000_000);
    }

    // -- log_timing --------------------------------------------------------

    #[test]
    fn log_timing_format() {
        let tp = TimingPoint {
            index: 1,
            perf_usec: 100,
            duration_usec: 200,
            period_usec: 1000,
            start_time_nsec: 5000,
            end_time_nsec: 5200,
            rel_start_time_nsec: 200,
            slack_nsec: -50,
            cumulative_duration_usec: 300,
            cumulative_period_usec: 2000,
            wakeup_latency_usec: 10,
        };
        let mut buf = Vec::new();
        log_timing(&mut buf, &tp).unwrap();
        let output = String::from_utf8(buf).unwrap();
        // Should contain the values in order
        assert!(output.contains("1"));
        assert!(output.contains("100"));
        assert!(output.contains("200"));
        assert!(output.contains("1000"));
        assert!(output.contains("5000"));
        assert!(output.contains("5200"));
        assert!(output.contains("-50"));
        assert!(output.ends_with('\n'));
    }

    // -- gettid ------------------------------------------------------------

    #[test]
    fn gettid_returns_positive() {
        let tid = gettid();
        assert!(tid.as_raw() > 0);
    }

    // -- ftrace_parse ------------------------------------------------------

    #[test]
    fn ftrace_parse_single() {
        let level = ftrace_parse("main").unwrap();
        assert_eq!(level, FtraceLevel::MAIN);
    }

    #[test]
    fn ftrace_parse_multiple() {
        let level = ftrace_parse("main,task,loop").unwrap();
        assert!(level.contains(FtraceLevel::MAIN));
        assert!(level.contains(FtraceLevel::TASK));
        assert!(level.contains(FtraceLevel::LOOP));
        assert!(!level.contains(FtraceLevel::EVENT));
    }

    #[test]
    fn ftrace_parse_case_insensitive() {
        let level = ftrace_parse("MAIN,Task,LOOP").unwrap();
        assert!(level.contains(FtraceLevel::MAIN));
        assert!(level.contains(FtraceLevel::TASK));
        assert!(level.contains(FtraceLevel::LOOP));
    }

    #[test]
    fn ftrace_parse_none_resets() {
        let level = ftrace_parse("main,task,none").unwrap();
        assert_eq!(level, FtraceLevel::empty());
    }

    #[test]
    fn ftrace_parse_unknown_returns_error() {
        let result = ftrace_parse("main,bogus");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("bogus"));
    }

    #[test]
    fn ftrace_parse_empty() {
        let level = ftrace_parse("").unwrap();
        assert_eq!(level, FtraceLevel::empty());
    }

    #[test]
    fn ftrace_parse_whitespace_tolerance() {
        let level = ftrace_parse("main , task , loop").unwrap();
        assert!(level.contains(FtraceLevel::MAIN));
        assert!(level.contains(FtraceLevel::TASK));
        assert!(level.contains(FtraceLevel::LOOP));
    }

    // -- ftrace_write ------------------------------------------------------

    #[test]
    fn ftrace_write_to_vec() {
        let mut buf = Vec::new();
        ftrace_write(&mut buf, "hello ftrace").unwrap();
        assert_eq!(buf, b"hello ftrace");
    }

    #[test]
    fn ftrace_write_fmt_to_vec() {
        let mut buf = Vec::new();
        ftrace_write_fmt(&mut buf, format_args!("thread {} loop {}", 1, 42)).unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "thread 1 loop 42");
    }
}
