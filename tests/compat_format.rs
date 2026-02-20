//! Compatibility format verification tests.
//!
//! Ensures the Rust port produces output formats identical to the C original:
//! - Log file column layout (`log_timing` format)
//! - Log file header format
//! - Gnuplot script syntax validity
//! - Ftrace marker format strings
//! - Example JSON config parsing compatibility

use std::path::Path;

use rt_app_rs::config::parse_config_str;
use rt_app_rs::gnuplot::{self, ThreadPlotInfo};
use rt_app_rs::types::{FtraceLevel, TimingPoint};
use rt_app_rs::utils::{ftrace_write, ftrace_write_fmt, log_timing};

// ---------------------------------------------------------------------------
// Shared test infrastructure
// ---------------------------------------------------------------------------

/// Path to the C original's example configs (used as ground truth).
const C_ORIG_EXAMPLES: &str = "/home/newton/work/rt-app-rs/rt-app-orig/doc/examples";

/// Path to the local copy of example configs.
const LOCAL_EXAMPLES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/doc/examples");

/// JSON configs that use workgen-only extensions (bare keys without values)
/// and cannot be parsed by a standard JSON parser. These are tested
/// separately to verify they produce an appropriate error.
const WORKGEN_ONLY_CONFIGS: &[&str] = &["video-long.json", "video-short.json"];

/// Create a [`TimingPoint`] matching representative C output values.
fn sample_timing_point() -> TimingPoint {
    TimingPoint {
        index: 42,
        perf_usec: 9876,
        duration_usec: 10000,
        period_usec: 100000,
        start_time_nsec: 1_234_567_890_123,
        end_time_nsec: 1_234_567_900_123,
        rel_start_time_nsec: 567_890_123,
        slack_nsec: -5000,
        cumulative_duration_usec: 420000,
        cumulative_period_usec: 4200000,
        wakeup_latency_usec: 15,
    }
}

/// Render a [`TimingPoint`] to a string via `log_timing`.
fn render_timing(tp: &TimingPoint) -> String {
    let mut buf = Vec::new();
    log_timing(&mut buf, tp).unwrap();
    String::from_utf8(buf).unwrap()
}

/// Parse a log line into its whitespace-separated columns.
fn parse_log_columns(line: &str) -> Vec<String> {
    line.split_whitespace().map(String::from).collect()
}

/// Verify a gnuplot script contains all required directives.
fn assert_valid_gnuplot_script(content: &str, context: &str) {
    let required = [
        ("set terminal", "terminal type"),
        ("set output", "output file"),
        ("plot ", "plot command"),
        ("set xlabel", "x-axis label"),
        ("set ylabel", "y-axis label"),
        ("set grid", "grid"),
        ("set title", "title"),
        ("replot", "replot trailer"),
    ];
    for (needle, desc) in required {
        assert!(
            content.contains(needle),
            "{context}: missing '{desc}' ({needle})"
        );
    }
    for line in content.lines() {
        if line.starts_with("set ") {
            let quote_count = line.chars().filter(|&c| c == '"').count();
            assert!(
                quote_count % 2 == 0,
                "{context}: unbalanced quotes in: {line}"
            );
        }
    }
}

/// Check if a config filename is a workgen-only config.
fn is_workgen_only(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|name| WORKGEN_ONLY_CONFIGS.contains(&name))
}

/// Load and parse a JSON config.
fn parse_example_config(path: &Path) -> Result<(), String> {
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    parse_config_str(&content)
        .map(|_| ())
        .map_err(|e| format!("parse {}: {e}", path.display()))
}

/// Collect all JSON files under a directory (one level of nesting).
fn collect_json_configs(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut configs = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json") {
                configs.push(path);
            } else if path.is_dir() {
                if let Ok(sub_entries) = std::fs::read_dir(&path) {
                    for sub in sub_entries.flatten() {
                        let sub_path = sub.path();
                        if sub_path.extension().is_some_and(|e| e == "json") {
                            configs.push(sub_path);
                        }
                    }
                }
            }
        }
    }
    configs.sort();
    configs
}

// ===========================================================================
// A) Log format compatibility tests
// ===========================================================================

mod log_format {
    use super::*;

    /// The C format string is:
    /// `"%4d %8lu %8lu %8lu %15llu %15llu %15llu %10ld %10lu %10lu %10lu\n"`
    const EXPECTED_COLUMN_COUNT: usize = 11;
    const EXPECTED_FIELD_WIDTHS: [usize; 11] = [4, 8, 8, 8, 15, 15, 15, 10, 10, 10, 10];

    #[test]
    fn produces_exactly_11_columns() {
        let line = render_timing(&sample_timing_point());
        let cols = parse_log_columns(line.trim());
        assert_eq!(cols.len(), EXPECTED_COLUMN_COUNT);
    }

    #[test]
    fn field_order_matches_c() {
        let tp = sample_timing_point();
        let line = render_timing(&tp);
        let cols = parse_log_columns(line.trim());

        let expected: Vec<String> = vec![
            tp.index.to_string(),
            tp.perf_usec.to_string(),
            tp.duration_usec.to_string(),
            tp.period_usec.to_string(),
            tp.start_time_nsec.to_string(),
            tp.end_time_nsec.to_string(),
            tp.rel_start_time_nsec.to_string(),
            tp.slack_nsec.to_string(),
            tp.cumulative_duration_usec.to_string(),
            tp.cumulative_period_usec.to_string(),
            tp.wakeup_latency_usec.to_string(),
        ];
        assert_eq!(cols, expected);
    }

    #[test]
    fn column_widths_match_c_format() {
        let tp = TimingPoint {
            index: 1,
            perf_usec: 10,
            duration_usec: 20,
            period_usec: 30,
            start_time_nsec: 40,
            end_time_nsec: 50,
            rel_start_time_nsec: 60,
            slack_nsec: -7,
            cumulative_duration_usec: 80,
            cumulative_period_usec: 90,
            wakeup_latency_usec: 11,
        };
        let line = render_timing(&tp);
        let line = line.trim_end_matches('\n');

        let expected_len: usize =
            EXPECTED_FIELD_WIDTHS.iter().sum::<usize>() + EXPECTED_FIELD_WIDTHS.len() - 1;
        assert_eq!(
            line.len(),
            expected_len,
            "line length: expected {expected_len}, got {}.\nLine: '{line}'",
            line.len()
        );
    }

    #[test]
    fn negative_slack_preserves_sign() {
        let tp = TimingPoint {
            slack_nsec: -12345,
            ..TimingPoint::default()
        };
        let line = render_timing(&tp);
        let cols = parse_log_columns(line.trim());
        assert_eq!(cols[7], "-12345");
    }

    #[test]
    fn zero_values_all_zero() {
        let tp = TimingPoint::default();
        let line = render_timing(&tp);
        let cols = parse_log_columns(line.trim());
        assert_eq!(cols.len(), EXPECTED_COLUMN_COUNT);
        for (i, col) in cols.iter().enumerate() {
            assert_eq!(col, "0", "field {i} should be 0 for default");
        }
    }

    #[test]
    fn large_values_not_truncated() {
        let tp = TimingPoint {
            index: 9999,
            perf_usec: 99999999,
            duration_usec: 99999999,
            period_usec: 99999999,
            start_time_nsec: u64::MAX,
            end_time_nsec: u64::MAX,
            rel_start_time_nsec: u64::MAX,
            slack_nsec: i64::MIN,
            cumulative_duration_usec: u64::MAX,
            cumulative_period_usec: u64::MAX,
            wakeup_latency_usec: u64::MAX,
        };
        let line = render_timing(&tp);
        let cols = parse_log_columns(line.trim());
        assert_eq!(cols.len(), EXPECTED_COLUMN_COUNT);
        assert_eq!(cols[0], "9999");
        assert_eq!(cols[4], u64::MAX.to_string());
        assert_eq!(cols[7], i64::MIN.to_string());
    }

    #[test]
    fn ends_with_single_newline() {
        let output = render_timing(&TimingPoint::default());
        assert!(output.ends_with('\n'));
        assert!(!output.ends_with("\n\n"));
    }
}

// ===========================================================================
// B) Log header format tests
// ===========================================================================

mod log_header {
    use rt_app_rs::engine::timing::write_log_header;

    #[test]
    fn starts_with_comment_marker() {
        let mut buf = Vec::new();
        write_log_header(&mut buf).unwrap();
        let header = String::from_utf8(buf).unwrap();
        assert!(header.starts_with("#idx"));
    }

    #[test]
    fn contains_all_column_names() {
        let mut buf = Vec::new();
        write_log_header(&mut buf).unwrap();
        let header = String::from_utf8(buf).unwrap();
        for name in [
            "perf",
            "run",
            "period",
            "start",
            "end",
            "rel_st",
            "slack",
            "c_duration",
            "c_period",
            "wu_lat",
        ] {
            assert!(header.contains(name), "header missing: {name}");
        }
    }

    #[test]
    fn has_11_columns() {
        let mut buf = Vec::new();
        write_log_header(&mut buf).unwrap();
        let header = String::from_utf8(buf).unwrap();
        let cols: Vec<&str> = header.trim().split_whitespace().collect();
        assert_eq!(cols.len(), 11);
    }

    #[test]
    fn ends_with_newline() {
        let mut buf = Vec::new();
        write_log_header(&mut buf).unwrap();
        let header = String::from_utf8(buf).unwrap();
        assert!(header.ends_with('\n'));
    }
}

// ===========================================================================
// C) Gnuplot script validity tests
// ===========================================================================

mod gnuplot_compat {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn thread_script_valid_syntax() {
        let dir = TempDir::new().unwrap();
        gnuplot::setup_thread_gnuplot(dir.path(), "rt-app", "worker0").unwrap();
        let content = std::fs::read_to_string(dir.path().join("rt-app-worker0.plot")).unwrap();
        assert_valid_gnuplot_script(&content, "thread gnuplot");
    }

    #[test]
    fn thread_script_references_correct_log() {
        let dir = TempDir::new().unwrap();
        gnuplot::setup_thread_gnuplot(dir.path(), "myapp", "t0").unwrap();
        let content = std::fs::read_to_string(dir.path().join("myapp-t0.plot")).unwrap();
        assert!(content.contains("\"myapp-t0.log\""));
    }

    #[test]
    fn thread_script_correct_columns() {
        let dir = TempDir::new().unwrap();
        gnuplot::setup_thread_gnuplot(dir.path(), "rt-app", "t0").unwrap();
        let content = std::fs::read_to_string(dir.path().join("rt-app-t0.plot")).unwrap();
        assert!(content.contains(":2"), "must plot column 2");
        assert!(content.contains(":3"), "must plot column 3");
        assert!(content.contains(":4"), "must plot column 4");
        assert!(content.contains("$5/1000000"), "x-axis nsec->msec");
    }

    #[test]
    fn thread_script_eps_output() {
        let dir = TempDir::new().unwrap();
        gnuplot::setup_thread_gnuplot(dir.path(), "rt-app", "thr").unwrap();
        let content = std::fs::read_to_string(dir.path().join("rt-app-thr.plot")).unwrap();
        assert!(content.contains("postscript enhanced color"));
        assert!(content.contains("'rt-app-thr.eps'"));
    }

    #[test]
    fn thread_script_y2_axis_for_load() {
        let dir = TempDir::new().unwrap();
        gnuplot::setup_thread_gnuplot(dir.path(), "rt-app", "w").unwrap();
        let content = std::fs::read_to_string(dir.path().join("rt-app-w.plot")).unwrap();
        assert!(content.contains("axes x1y2"));
        assert!(content.contains("set y2label"));
        assert!(content.contains("set y2tics"));
    }

    #[test]
    fn main_scripts_valid_syntax() {
        let dir = TempDir::new().unwrap();
        let threads = vec![
            ThreadPlotInfo {
                name: "alpha",
                policy: "SCHED_OTHER",
            },
            ThreadPlotInfo {
                name: "beta",
                policy: "SCHED_FIFO",
            },
        ];
        gnuplot::setup_main_gnuplot(dir.path(), "rt-app", &threads).unwrap();

        for suffix in ["period", "run"] {
            let content =
                std::fs::read_to_string(dir.path().join(format!("rt-app-{suffix}.plot"))).unwrap();
            assert_valid_gnuplot_script(&content, &format!("main {suffix}"));
        }
    }

    #[test]
    fn main_scripts_use_usec_x_axis() {
        let dir = TempDir::new().unwrap();
        let threads = vec![ThreadPlotInfo {
            name: "t",
            policy: "SCHED_OTHER",
        }];
        gnuplot::setup_main_gnuplot(dir.path(), "rt-app", &threads).unwrap();

        let content = std::fs::read_to_string(dir.path().join("rt-app-period.plot")).unwrap();
        assert!(content.contains("$5/1000"), "aggregate x-axis nsec->usec");
        assert!(content.contains("Loop start time [usec]"));
    }

    #[test]
    fn main_scripts_legends_include_policy() {
        let dir = TempDir::new().unwrap();
        let threads = vec![
            ThreadPlotInfo {
                name: "w0",
                policy: "SCHED_DEADLINE",
            },
            ThreadPlotInfo {
                name: "w1",
                policy: "SCHED_RR",
            },
        ];
        gnuplot::setup_main_gnuplot(dir.path(), "rt-app", &threads).unwrap();

        let content = std::fs::read_to_string(dir.path().join("rt-app-run.plot")).unwrap();
        assert!(content.contains("thread [w0] (SCHED_DEADLINE)"));
        assert!(content.contains("thread [w1] (SCHED_RR)"));
    }

    #[test]
    fn main_scripts_no_trailing_comma_single() {
        let dir = TempDir::new().unwrap();
        let threads = vec![ThreadPlotInfo {
            name: "solo",
            policy: "SCHED_FIFO",
        }];
        gnuplot::setup_main_gnuplot(dir.path(), "rt-app", &threads).unwrap();

        let content = std::fs::read_to_string(dir.path().join("rt-app-period.plot")).unwrap();
        assert!(!content.contains("SCHED_FIFO)\", "));
    }
}

// ===========================================================================
// D) Ftrace marker format tests
// ===========================================================================

mod ftrace_compat {
    use super::*;

    #[test]
    fn level_bits_match_c_defines() {
        assert_eq!(FtraceLevel::empty().bits(), 0x00);
        assert_eq!(FtraceLevel::MAIN.bits(), 0x01);
        assert_eq!(FtraceLevel::TASK.bits(), 0x02);
        assert_eq!(FtraceLevel::LOOP.bits(), 0x04);
        assert_eq!(FtraceLevel::EVENT.bits(), 0x08);
        assert_eq!(FtraceLevel::STATS.bits(), 0x10);
        assert_eq!(FtraceLevel::ATTRS.bits(), 0x20);
    }

    #[test]
    fn write_produces_exact_bytes() {
        let mut buf = Vec::new();
        ftrace_write(&mut buf, "rtapp_main: event=start").unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "rtapp_main: event=start");
    }

    #[test]
    fn write_fmt_loop_format() {
        let mut buf = Vec::new();
        let (tl, ph, pl) = (5, 2, 3);
        ftrace_write_fmt(
            &mut buf,
            format_args!("rtapp_loop: event=start thread_loop={tl} phase={ph} phase_loop={pl}"),
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "rtapp_loop: event=start thread_loop=5 phase=2 phase_loop=3"
        );
    }

    #[test]
    fn write_fmt_stats_format() {
        let mut buf = Vec::new();
        let (period, run, wu_lat, slack, c_period, c_run) =
            (100000, 10000, 15, -5000, 200000, 20000);
        ftrace_write_fmt(
            &mut buf,
            format_args!(
                "rtapp_stats: period={period} run={run} wu_lat={wu_lat} \
                 slack={slack} c_period={c_period} c_run={c_run}"
            ),
        )
        .unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.starts_with("rtapp_stats:"));
        assert!(output.contains("period=100000"));
        assert!(output.contains("slack=-5000"));
    }

    #[test]
    fn write_fmt_event_format() {
        let mut buf = Vec::new();
        let (id, etype, desc) = (3, 6, "run0");
        ftrace_write_fmt(
            &mut buf,
            format_args!("rtapp_event: id={id} type={etype} desc={desc}"),
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "rtapp_event: id=3 type=6 desc=run0"
        );
    }

    #[test]
    fn write_fmt_attrs_format() {
        let mut buf = Vec::new();
        ftrace_write_fmt(
            &mut buf,
            format_args!("rtapp_attrs: event=policy policy=SCHED_FIFO prio=10"),
        )
        .unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.starts_with("rtapp_attrs:"));
        assert!(output.contains("SCHED_FIFO"));
    }
}

// ===========================================================================
// E) Example JSON config parsing compatibility
// ===========================================================================

mod config_parsing {
    use super::*;

    #[test]
    fn parse_all_tutorial_examples() {
        let tutorial_dir = Path::new(LOCAL_EXAMPLES).join("tutorial");
        let configs = collect_json_configs(&tutorial_dir);
        assert!(!configs.is_empty(), "no tutorial configs found");
        for config in &configs {
            parse_example_config(config).unwrap_or_else(|e| panic!("{e}"));
        }
    }

    #[test]
    fn parse_standard_top_level_examples() {
        let dir = Path::new(LOCAL_EXAMPLES);
        let mut configs: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "json"))
            .filter(|p| !is_workgen_only(p))
            .collect();
        configs.sort();
        assert!(!configs.is_empty(), "no standard top-level configs found");
        for config in &configs {
            parse_example_config(config).unwrap_or_else(|e| panic!("{e}"));
        }
    }

    #[test]
    fn workgen_only_configs_produce_parse_error() {
        let dir = Path::new(LOCAL_EXAMPLES);
        for name in WORKGEN_ONLY_CONFIGS {
            let path = dir.join(name);
            if path.exists() {
                let result = parse_example_config(&path);
                assert!(
                    result.is_err(),
                    "workgen-only config {name} should fail standard parsing"
                );
            }
        }
    }

    #[test]
    fn example1_structure_matches_c() {
        let content =
            std::fs::read_to_string(Path::new(LOCAL_EXAMPLES).join("tutorial/example1.json"))
                .unwrap();
        let config = parse_config_str(&content).unwrap();

        assert_eq!(config.tasks.len(), 1);
        assert_eq!(config.tasks[0].name, "thread0");
        assert_eq!(config.global.duration_secs, 2);
        assert!(config.global.gnuplot);
        let ft = config.global.ftrace.0;
        assert!(ft.contains(FtraceLevel::MAIN));
        assert!(ft.contains(FtraceLevel::TASK));
        assert!(ft.contains(FtraceLevel::LOOP));
        assert!(ft.contains(FtraceLevel::EVENT));
    }

    #[test]
    fn example3_phases_match_c() {
        let content =
            std::fs::read_to_string(Path::new(LOCAL_EXAMPLES).join("tutorial/example3.json"))
                .unwrap();
        let config = parse_config_str(&content).unwrap();

        assert_eq!(config.tasks.len(), 1);
        assert_eq!(config.tasks[0].phases.len(), 2);
        assert_eq!(config.tasks[0].num_instances, 12);
    }

    #[test]
    fn example7_barriers_match_c() {
        let content =
            std::fs::read_to_string(Path::new(LOCAL_EXAMPLES).join("tutorial/example7.json"))
                .unwrap();
        let config = parse_config_str(&content).unwrap();

        assert_eq!(config.tasks.len(), 2);
        let barrier_count = config.tasks[0].phases[0]
            .events
            .iter()
            .filter(|e| e.event_type == rt_app_rs::types::ResourceType::Barrier)
            .count();
        assert_eq!(barrier_count, 3);
    }

    #[test]
    fn example9_fork_match_c() {
        let content =
            std::fs::read_to_string(Path::new(LOCAL_EXAMPLES).join("tutorial/example9.json"))
                .unwrap();
        let config = parse_config_str(&content).unwrap();

        assert_eq!(config.tasks.len(), 3);
        let t2 = config.tasks.iter().find(|t| t.name == "thread2").unwrap();
        assert_eq!(t2.num_instances, 0);
    }

    #[test]
    fn local_tutorial_count_matches_c_orig() {
        let c_tutorial = Path::new(C_ORIG_EXAMPLES).join("tutorial");
        if !c_tutorial.exists() {
            return;
        }
        let local_tutorial = Path::new(LOCAL_EXAMPLES).join("tutorial");

        let c_count = collect_json_configs(&c_tutorial).len();
        let local_count = collect_json_configs(&local_tutorial).len();
        assert_eq!(
            c_count, local_count,
            "tutorial config count: C={c_count}, local={local_count}"
        );
    }
}

// ===========================================================================
// F) Cross-format consistency tests
// ===========================================================================

mod cross_format {
    use super::*;
    use rt_app_rs::engine::timing::write_log_header;

    #[test]
    fn header_and_data_column_count_match() {
        let mut hdr_buf = Vec::new();
        write_log_header(&mut hdr_buf).unwrap();
        let hdr_str = String::from_utf8(hdr_buf).unwrap();
        let hdr_cols: Vec<&str> = hdr_str.trim().split_whitespace().collect();

        let data_str = render_timing(&sample_timing_point());
        let data_cols = parse_log_columns(data_str.trim());

        assert_eq!(hdr_cols.len(), data_cols.len());
    }

    #[test]
    fn gnuplot_column_refs_within_log_range() {
        let line = render_timing(&TimingPoint::default());
        let max_col = parse_log_columns(line.trim()).len();

        for col_ref in [2, 3, 4, 5] {
            assert!(
                col_ref <= max_col,
                "gnuplot column {col_ref} exceeds data columns ({max_col})"
            );
        }
    }
}
