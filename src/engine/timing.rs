//! Timing data collection and log writing for rt-app threads.
//!
//! Ported from the timing/logging portions of `thread_body()` in `rt-app.c`.
//! Provides the circular timing buffer and gnuplot output generation.

use std::io::{self, Write};
use std::path::Path;

use crate::types::TimingPoint;
use crate::utils::log_timing;

// ---------------------------------------------------------------------------
// TimingBuffer — circular buffer for timing points
// ---------------------------------------------------------------------------

/// A fixed-size circular buffer of [`TimingPoint`] measurements.
///
/// When the buffer fills, new entries overwrite the oldest. This matches
/// the C original's `timings` array with wrap-around tracking via
/// `log_idx` and `timing_loop`.
pub struct TimingBuffer {
    buf: Vec<TimingPoint>,
    /// Next write position.
    write_idx: usize,
    /// Whether we have wrapped around at least once.
    wrapped: bool,
}

impl TimingBuffer {
    /// Create a new buffer that can hold `capacity` timing points.
    ///
    /// Returns `None` if `capacity` is 0 (logging disabled).
    pub fn new(capacity: usize) -> Option<Self> {
        if capacity == 0 {
            return None;
        }
        Some(Self {
            buf: vec![TimingPoint::default(); capacity],
            write_idx: 0,
            wrapped: false,
        })
    }

    /// Record a new timing point. Overwrites the oldest if full.
    pub fn push(&mut self, point: TimingPoint) {
        self.buf[self.write_idx] = point;
        self.write_idx += 1;
        if self.write_idx >= self.buf.len() {
            self.wrapped = true;
            self.write_idx = 0;
        }
    }

    /// Iterate over stored timing points in chronological order.
    ///
    /// If the buffer has wrapped, the oldest entries start at `write_idx`.
    pub fn iter_chronological(&self) -> impl Iterator<Item = &TimingPoint> {
        let len = self.buf.len();
        let (first_range, second_range) = if self.wrapped {
            // Oldest is at write_idx..len, then 0..write_idx.
            (self.write_idx..len, 0..self.write_idx)
        } else {
            // No wrap: entries are at 0..write_idx.
            (0..0, 0..self.write_idx)
        };
        self.buf[first_range]
            .iter()
            .chain(self.buf[second_range].iter())
    }

    /// Flush all stored timing points to the given writer.
    pub fn flush_to<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        for point in self.iter_chronological() {
            log_timing(writer, point)?;
        }
        Ok(())
    }

    /// Number of entries stored (up to capacity).
    pub fn len(&self) -> usize {
        if self.wrapped {
            self.buf.len()
        } else {
            self.write_idx
        }
    }

    /// Whether no entries have been recorded.
    pub fn is_empty(&self) -> bool {
        !self.wrapped && self.write_idx == 0
    }
}

// ---------------------------------------------------------------------------
// Log header
// ---------------------------------------------------------------------------

/// Write the column header line to a log file.
pub fn write_log_header<W: Write>(writer: &mut W) -> io::Result<()> {
    writeln!(
        writer,
        "#idx {:>8} {:>8} {:>8} {:>15} {:>15} {:>15} {:>10} {:>10} {:>10} {:>10}",
        "perf",
        "run",
        "period",
        "start",
        "end",
        "rel_st",
        "slack",
        "c_duration",
        "c_period",
        "wu_lat"
    )
}

// ---------------------------------------------------------------------------
// Gnuplot generation
// ---------------------------------------------------------------------------

/// Generate a per-thread gnuplot script.
pub fn write_thread_gnuplot(logdir: &Path, logbasename: &str, thread_name: &str) -> io::Result<()> {
    let plot_path = logdir.join(format!("{logbasename}-{thread_name}.plot"));
    let eps_name = format!("{logbasename}-{thread_name}.eps");
    let log_name = format!("{logbasename}-{thread_name}.log");

    let mut f = std::fs::File::create(&plot_path)?;
    writeln!(f, "set terminal postscript enhanced color")?;
    writeln!(f, "set output '{eps_name}'")?;
    writeln!(f, "set grid")?;
    writeln!(f, "set key outside right")?;
    writeln!(f, "set title \"Measured {thread_name} Loop stats\"")?;
    writeln!(f, "set xlabel \"Loop start time [msec]\"")?;
    writeln!(f, "set ylabel \"Period/Run Time [usec]\"")?;
    writeln!(f, "set y2label \"Load [number of 1000 loops executed]\"")?;
    writeln!(f, "set y2tics")?;
    writeln!(f, "set xtics rotate by -45")?;
    writeln!(
        f,
        "plot \"{log_name}\" u ($5/1000000):2 w l title \"load \" axes x1y2, \
         \"{log_name}\" u ($5/1000000):3 w l title \"run \", \
         \"{log_name}\" u ($5/1000000):4 w l title \"period \""
    )?;
    writeln!(f, "set terminal wxt")?;
    writeln!(f, "replot")?;
    Ok(())
}

/// Thread info for gnuplot period/run scripts.
pub struct ThreadPlotInfo<'a> {
    pub name: &'a str,
    pub policy_str: &'a str,
}

/// Generate the main gnuplot scripts (period and run plots across all threads).
pub fn write_main_gnuplot(
    logdir: &Path,
    logbasename: &str,
    threads: &[ThreadPlotInfo<'_>],
) -> io::Result<()> {
    write_main_gnuplot_file(
        logdir,
        logbasename,
        threads,
        "period",
        "Measured time per loop",
        "Period Time [usec]",
        4, // column index for period
    )?;
    write_main_gnuplot_file(
        logdir,
        logbasename,
        threads,
        "run",
        "Measured run time per loop",
        "Run Time [usec]",
        3, // column index for run
    )?;
    Ok(())
}

fn write_main_gnuplot_file(
    logdir: &Path,
    logbasename: &str,
    threads: &[ThreadPlotInfo<'_>],
    suffix: &str,
    title: &str,
    ylabel: &str,
    col: u32,
) -> io::Result<()> {
    let plot_path = logdir.join(format!("{logbasename}-{suffix}.plot"));
    let eps_name = format!("{logbasename}-{suffix}.eps");

    let mut f = std::fs::File::create(&plot_path)?;
    writeln!(f, "set terminal postscript enhanced color")?;
    writeln!(f, "set output '{eps_name}'")?;
    writeln!(f, "set grid")?;
    writeln!(f, "set key outside right")?;
    writeln!(f, "set title \"{title}\"")?;
    writeln!(f, "set xlabel \"Loop start time [usec]\"")?;
    writeln!(f, "set ylabel \"{ylabel}\"")?;
    writeln!(f, "set xtics rotate by -45")?;
    writeln!(f, "set key noenhanced")?;
    write!(f, "plot ")?;

    for (i, t) in threads.iter().enumerate() {
        let log_name = format!("{logbasename}-{}.log", t.name);
        write!(
            f,
            "\"{log_name}\" u ($5/1000):{col} w l title \"thread [{}] ({})\"",
            t.name, t.policy_str
        )?;
        if i < threads.len() - 1 {
            write!(f, ", ")?;
        }
    }
    writeln!(f)?;
    writeln!(f, "set terminal wxt")?;
    writeln!(f, "replot")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_point(index: u32) -> TimingPoint {
        TimingPoint {
            index,
            ..TimingPoint::default()
        }
    }

    #[test]
    fn timing_buffer_none_for_zero() {
        assert!(TimingBuffer::new(0).is_none());
    }

    #[test]
    fn timing_buffer_push_and_len() {
        let mut tb = TimingBuffer::new(4).unwrap();
        assert!(tb.is_empty());
        assert_eq!(tb.len(), 0);

        tb.push(make_point(1));
        assert_eq!(tb.len(), 1);
        assert!(!tb.is_empty());

        tb.push(make_point(2));
        tb.push(make_point(3));
        assert_eq!(tb.len(), 3);
    }

    #[test]
    fn timing_buffer_wrap_around() {
        let mut tb = TimingBuffer::new(3).unwrap();
        tb.push(make_point(1));
        tb.push(make_point(2));
        tb.push(make_point(3));
        assert_eq!(tb.len(), 3);

        // This wraps around, overwriting point 1.
        tb.push(make_point(4));
        assert_eq!(tb.len(), 3);

        let indices: Vec<u32> = tb.iter_chronological().map(|p| p.index).collect();
        assert_eq!(indices, vec![2, 3, 4]);
    }

    #[test]
    fn timing_buffer_no_wrap_order() {
        let mut tb = TimingBuffer::new(5).unwrap();
        tb.push(make_point(10));
        tb.push(make_point(20));

        let indices: Vec<u32> = tb.iter_chronological().map(|p| p.index).collect();
        assert_eq!(indices, vec![10, 20]);
    }

    #[test]
    fn timing_buffer_flush() {
        let mut tb = TimingBuffer::new(2).unwrap();
        tb.push(make_point(1));
        tb.push(make_point(2));

        let mut output = Vec::new();
        tb.flush_to(&mut output).unwrap();
        let text = String::from_utf8(output).unwrap();
        // Should have 2 lines (one per point).
        assert_eq!(text.lines().count(), 2);
    }

    #[test]
    fn write_log_header_format() {
        let mut buf = Vec::new();
        write_log_header(&mut buf).unwrap();
        let header = String::from_utf8(buf).unwrap();
        assert!(header.contains("#idx"));
        assert!(header.contains("perf"));
        assert!(header.contains("period"));
        assert!(header.contains("slack"));
    }

    #[test]
    fn thread_gnuplot_creates_file() {
        let tmp = tempfile::tempdir().unwrap();
        write_thread_gnuplot(tmp.path(), "test", "thread0").unwrap();
        let plot = tmp.path().join("test-thread0.plot");
        assert!(plot.exists());
        let content = std::fs::read_to_string(plot).unwrap();
        assert!(content.contains("thread0"));
        assert!(content.contains("postscript"));
    }

    #[test]
    fn main_gnuplot_creates_files() {
        let tmp = tempfile::tempdir().unwrap();
        let threads = vec![
            ThreadPlotInfo {
                name: "t1",
                policy_str: "SCHED_OTHER",
            },
            ThreadPlotInfo {
                name: "t2",
                policy_str: "SCHED_FIFO",
            },
        ];
        write_main_gnuplot(tmp.path(), "test", &threads).unwrap();

        let period_plot = tmp.path().join("test-period.plot");
        let run_plot = tmp.path().join("test-run.plot");
        assert!(period_plot.exists());
        assert!(run_plot.exists());

        let content = std::fs::read_to_string(period_plot).unwrap();
        assert!(content.contains("t1"));
        assert!(content.contains("t2"));
        assert!(content.contains("SCHED_FIFO"));
    }
}
