//! Gnuplot script generation and per-thread log file setup.
//!
//! Ported from `setup_thread_logging()`, `setup_thread_gnuplot()`, and
//! `setup_main_gnuplot()` in the C `rt-app.c`.

use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Path construction helpers
// ---------------------------------------------------------------------------

/// Build the path `<logdir>/<log_basename>-<thread_name>.<ext>`.
fn thread_file_path(logdir: &Path, log_basename: &str, thread_name: &str, ext: &str) -> PathBuf {
    logdir.join(format!("{log_basename}-{thread_name}.{ext}"))
}

/// Build the path `<logdir>/<log_basename>-<suffix>.<ext>`.
fn aggregate_file_path(logdir: &Path, log_basename: &str, suffix: &str, ext: &str) -> PathBuf {
    logdir.join(format!("{log_basename}-{suffix}.{ext}"))
}

// ---------------------------------------------------------------------------
// Logging
// ---------------------------------------------------------------------------

/// Open a per-thread log file at `<logdir>/<log_basename>-<thread_name>.log`.
///
/// Returns the opened [`File`] handle ready for writing.
pub fn setup_thread_logging(
    logdir: &Path,
    log_basename: &str,
    thread_name: &str,
) -> io::Result<File> {
    let path = thread_file_path(logdir, log_basename, thread_name, "log");
    File::create(path)
}

// ---------------------------------------------------------------------------
// Per-thread gnuplot script
// ---------------------------------------------------------------------------

/// Write the common gnuplot preamble (terminal, output, grid, key, labels).
fn write_preamble(
    w: &mut impl Write,
    eps_name: &str,
    title: &str,
    xlabel: &str,
    ylabel: &str,
    extras: &[&str],
) -> io::Result<()> {
    writeln!(w, "set terminal postscript enhanced color")?;
    writeln!(w, "set output '{eps_name}'")?;
    writeln!(w, "set grid")?;
    writeln!(w, "set key outside right")?;
    writeln!(w, "set title \"{title}\"")?;
    writeln!(w, "set xlabel \"{xlabel}\"")?;
    writeln!(w, "set ylabel \"{ylabel}\"")?;
    for line in extras {
        writeln!(w, "{line}")?;
    }
    Ok(())
}

/// Write the replot trailer shared by all generated scripts.
fn write_trailer(w: &mut impl Write) -> io::Result<()> {
    writeln!(w, "set terminal wxt")?;
    writeln!(w, "replot")
}

/// Generate a per-thread gnuplot script (`.plot` file) that plots:
/// - Load (perf, column 2) vs loop start time (column 5)
/// - Run duration (column 3) vs loop start time
/// - Period (column 4) vs loop start time
///
/// The script produces PostScript EPS output matching the C original.
pub fn setup_thread_gnuplot(
    logdir: &Path,
    log_basename: &str,
    thread_name: &str,
) -> io::Result<()> {
    let plot_path = thread_file_path(logdir, log_basename, thread_name, "plot");
    let eps_name = format!("{log_basename}-{thread_name}.eps");
    let log_name = format!("{log_basename}-{thread_name}.log");

    let mut f = File::create(plot_path)?;

    write_preamble(
        &mut f,
        &eps_name,
        &format!("Measured {thread_name} Loop stats"),
        "Loop start time [msec]",
        "Period/Run Time [usec]",
        &[
            "set y2label \"Load [number of 1000 loops executed]\"",
            "set y2tics  ",
            "set xtics rotate by -45",
        ],
    )?;

    // plot line: load (axes x1y2), run, period
    // Column 5 is start_time_nsec; dividing by 1_000_000 gives msec.
    writeln!(
        f,
        "plot \
         \"{log_name}\" u ($5/1000000):2 w l title \"load \" axes x1y2, \
         \"{log_name}\" u ($5/1000000):3 w l title \"run \", \
         \"{log_name}\" u ($5/1000000):4 w l title \"period \"",
    )?;

    write_trailer(&mut f)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Aggregate (main) gnuplot scripts
// ---------------------------------------------------------------------------

/// Information about a thread needed for aggregate plot legends.
pub struct ThreadPlotInfo<'a> {
    pub name: &'a str,
    pub policy: &'a str,
}

/// Write the `plot` directive for an aggregate script, overlaying all threads.
///
/// `column` selects which data column to plot (3 = run, 4 = period).
/// Column 5 is start_time_nsec; dividing by 1000 gives usec (matching the C
/// original which uses usec for the aggregate x-axis).
fn write_aggregate_plot(
    w: &mut impl Write,
    log_basename: &str,
    threads: &[ThreadPlotInfo<'_>],
    column: u32,
) -> io::Result<()> {
    write!(w, "plot ")?;
    for (i, t) in threads.iter().enumerate() {
        let log_name = format!("{log_basename}-{}.log", t.name);
        write!(
            w,
            "\"{log_name}\" u ($5/1000):{column} w l title \"thread [{}] ({})\"",
            t.name, t.policy,
        )?;
        if i < threads.len() - 1 {
            write!(w, ", ")?;
        }
    }
    writeln!(w)?;
    Ok(())
}

/// Generate a single aggregate gnuplot script.
fn write_aggregate_script(
    logdir: &Path,
    log_basename: &str,
    suffix: &str,
    title: &str,
    ylabel: &str,
    threads: &[ThreadPlotInfo<'_>],
    column: u32,
) -> io::Result<()> {
    let plot_path = aggregate_file_path(logdir, log_basename, suffix, "plot");
    let eps_name = format!("{log_basename}-{suffix}.eps");

    let mut f = File::create(plot_path)?;

    write_preamble(
        &mut f,
        &eps_name,
        title,
        "Loop start time [usec]",
        ylabel,
        &["set xtics rotate by -45", "set key noenhanced"],
    )?;

    write_aggregate_plot(&mut f, log_basename, threads, column)?;
    write_trailer(&mut f)?;
    Ok(())
}

/// Generate TWO aggregate gnuplot scripts that overlay all threads:
/// - `<log_basename>-period.plot`: period (column 4) for all threads
/// - `<log_basename>-run.plot`: run duration (column 3) for all threads
pub fn setup_main_gnuplot(
    logdir: &Path,
    log_basename: &str,
    threads: &[ThreadPlotInfo<'_>],
) -> io::Result<()> {
    // Period plot (column 4)
    write_aggregate_script(
        logdir,
        log_basename,
        "period",
        "Measured time per loop",
        "Period Time [usec]",
        threads,
        4,
    )?;

    // Run plot (column 3)
    write_aggregate_script(
        logdir,
        log_basename,
        "run",
        "Measured run time per loop",
        "Run Time [usec]",
        threads,
        3,
    )?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // -- Path construction -------------------------------------------------

    #[test]
    fn thread_file_path_construction() {
        let p = thread_file_path(Path::new("/tmp/logs"), "rt-app", "thread0", "log");
        assert_eq!(p, PathBuf::from("/tmp/logs/rt-app-thread0.log"));
    }

    #[test]
    fn aggregate_file_path_construction() {
        let p = aggregate_file_path(Path::new("/tmp/logs"), "rt-app", "period", "plot");
        assert_eq!(p, PathBuf::from("/tmp/logs/rt-app-period.plot"));
    }

    // -- setup_thread_logging ----------------------------------------------

    #[test]
    fn setup_thread_logging_creates_file() {
        let dir = TempDir::new().unwrap();
        let f = setup_thread_logging(dir.path(), "rt-app", "worker1");
        assert!(f.is_ok());
        let expected = dir.path().join("rt-app-worker1.log");
        assert!(expected.exists());
    }

    #[test]
    fn setup_thread_logging_writable() {
        let dir = TempDir::new().unwrap();
        let mut f = setup_thread_logging(dir.path(), "rt-app", "t0").unwrap();
        writeln!(f, "test line").unwrap();
        drop(f);
        let contents = std::fs::read_to_string(dir.path().join("rt-app-t0.log")).unwrap();
        assert_eq!(contents, "test line\n");
    }

    // -- setup_thread_gnuplot ----------------------------------------------

    #[test]
    fn thread_gnuplot_creates_plot_file() {
        let dir = TempDir::new().unwrap();
        setup_thread_gnuplot(dir.path(), "rt-app", "thread0").unwrap();
        let plot_path = dir.path().join("rt-app-thread0.plot");
        assert!(plot_path.exists());
    }

    #[test]
    fn thread_gnuplot_contains_expected_commands() {
        let dir = TempDir::new().unwrap();
        setup_thread_gnuplot(dir.path(), "rt-app", "worker").unwrap();
        let contents = std::fs::read_to_string(dir.path().join("rt-app-worker.plot")).unwrap();

        assert!(contents.contains("set terminal postscript enhanced color"));
        assert!(contents.contains("set output 'rt-app-worker.eps'"));
        assert!(contents.contains("set grid"));
        assert!(contents.contains("set key outside right"));
        assert!(contents.contains("set title \"Measured worker Loop stats\""));
        assert!(contents.contains("set xlabel \"Loop start time [msec]\""));
        assert!(contents.contains("set ylabel \"Period/Run Time [usec]\""));
        assert!(contents.contains("set y2label \"Load [number of 1000 loops executed]\""));
        assert!(contents.contains("set y2tics"));
        assert!(contents.contains("set xtics rotate by -45"));
    }

    #[test]
    fn thread_gnuplot_plot_line_references() {
        let dir = TempDir::new().unwrap();
        setup_thread_gnuplot(dir.path(), "rt-app", "t1").unwrap();
        let contents = std::fs::read_to_string(dir.path().join("rt-app-t1.plot")).unwrap();

        // Load (perf) on y2 axis
        assert!(
            contents.contains("\"rt-app-t1.log\" u ($5/1000000):2 w l title \"load \" axes x1y2")
        );
        // Run duration
        assert!(contents.contains("\"rt-app-t1.log\" u ($5/1000000):3 w l title \"run \""));
        // Period
        assert!(contents.contains("\"rt-app-t1.log\" u ($5/1000000):4 w l title \"period \""));
    }

    #[test]
    fn thread_gnuplot_has_trailer() {
        let dir = TempDir::new().unwrap();
        setup_thread_gnuplot(dir.path(), "rt-app", "t0").unwrap();
        let contents = std::fs::read_to_string(dir.path().join("rt-app-t0.plot")).unwrap();

        assert!(contents.contains("set terminal wxt"));
        assert!(contents.contains("replot"));
    }

    // -- setup_main_gnuplot ------------------------------------------------

    #[test]
    fn main_gnuplot_creates_both_files() {
        let dir = TempDir::new().unwrap();
        let threads = vec![
            ThreadPlotInfo {
                name: "t0",
                policy: "SCHED_OTHER",
            },
            ThreadPlotInfo {
                name: "t1",
                policy: "SCHED_FIFO",
            },
        ];
        setup_main_gnuplot(dir.path(), "rt-app", &threads).unwrap();

        assert!(dir.path().join("rt-app-period.plot").exists());
        assert!(dir.path().join("rt-app-run.plot").exists());
    }

    #[test]
    fn main_gnuplot_period_plot_contents() {
        let dir = TempDir::new().unwrap();
        let threads = vec![
            ThreadPlotInfo {
                name: "alpha",
                policy: "SCHED_RR",
            },
            ThreadPlotInfo {
                name: "beta",
                policy: "SCHED_FIFO",
            },
        ];
        setup_main_gnuplot(dir.path(), "myapp", &threads).unwrap();

        let contents = std::fs::read_to_string(dir.path().join("myapp-period.plot")).unwrap();

        assert!(contents.contains("set terminal postscript enhanced color"));
        assert!(contents.contains("set output 'myapp-period.eps'"));
        assert!(contents.contains("set title \"Measured time per loop\""));
        assert!(contents.contains("set xlabel \"Loop start time [usec]\""));
        assert!(contents.contains("set ylabel \"Period Time [usec]\""));
        assert!(contents.contains("set key noenhanced"));

        // Thread-specific plot lines with column 4 (period)
        assert!(contents
            .contains("\"myapp-alpha.log\" u ($5/1000):4 w l title \"thread [alpha] (SCHED_RR)\""));
        assert!(contents
            .contains("\"myapp-beta.log\" u ($5/1000):4 w l title \"thread [beta] (SCHED_FIFO)\""));

        // Separator between threads
        assert!(contents.contains(", \"myapp-beta.log\""));

        assert!(contents.contains("set terminal wxt"));
        assert!(contents.contains("replot"));
    }

    #[test]
    fn main_gnuplot_run_plot_contents() {
        let dir = TempDir::new().unwrap();
        let threads = vec![
            ThreadPlotInfo {
                name: "w0",
                policy: "SCHED_OTHER",
            },
            ThreadPlotInfo {
                name: "w1",
                policy: "SCHED_DEADLINE",
            },
        ];
        setup_main_gnuplot(dir.path(), "bench", &threads).unwrap();

        let contents = std::fs::read_to_string(dir.path().join("bench-run.plot")).unwrap();

        assert!(contents.contains("set output 'bench-run.eps'"));
        assert!(contents.contains("set title \"Measured run time per loop\""));
        assert!(contents.contains("set ylabel \"Run Time [usec]\""));

        // Thread-specific plot lines with column 3 (run)
        assert!(contents
            .contains("\"bench-w0.log\" u ($5/1000):3 w l title \"thread [w0] (SCHED_OTHER)\""));
        assert!(contents
            .contains("\"bench-w1.log\" u ($5/1000):3 w l title \"thread [w1] (SCHED_DEADLINE)\""));
    }

    #[test]
    fn main_gnuplot_single_thread_no_trailing_comma() {
        let dir = TempDir::new().unwrap();
        let threads = vec![ThreadPlotInfo {
            name: "only",
            policy: "SCHED_FIFO",
        }];
        setup_main_gnuplot(dir.path(), "rt-app", &threads).unwrap();

        let contents = std::fs::read_to_string(dir.path().join("rt-app-period.plot")).unwrap();

        // The plot line should end with the title, followed by a newline, not a comma
        let plot_line_end = "title \"thread [only] (SCHED_FIFO)\"\n";
        assert!(contents.contains(plot_line_end));
        // No trailing comma after the single entry
        assert!(!contents.contains("SCHED_FIFO)\", "));
    }

    #[test]
    fn main_gnuplot_three_threads() {
        let dir = TempDir::new().unwrap();
        let threads = vec![
            ThreadPlotInfo {
                name: "a",
                policy: "SCHED_OTHER",
            },
            ThreadPlotInfo {
                name: "b",
                policy: "SCHED_FIFO",
            },
            ThreadPlotInfo {
                name: "c",
                policy: "SCHED_RR",
            },
        ];
        setup_main_gnuplot(dir.path(), "test", &threads).unwrap();

        let contents = std::fs::read_to_string(dir.path().join("test-period.plot")).unwrap();

        // All three threads present
        assert!(contents.contains("thread [a]"));
        assert!(contents.contains("thread [b]"));
        assert!(contents.contains("thread [c]"));

        // Commas separate first two, none after last
        assert!(contents.contains("(SCHED_OTHER)\", "));
        assert!(contents.contains("(SCHED_FIFO)\", "));
        assert!(!contents.contains("(SCHED_RR)\", "));
    }
}
