//! JSON configuration parser for rt-app-rs.
//!
//! Ported from `rt-app_parse_config.c` (1385 lines of manual json-c walking)
//! into idiomatic Rust using serde_json with derive-based deserialization and
//! custom deserializers for irregular JSON formats.
//!
//! The JSON configuration has three top-level keys:
//! - `"global"` (optional) — application-wide settings
//! - `"resources"` (optional) — explicit resource declarations
//! - `"tasks"` (required) — thread definitions with events and phases
//!
//! # Submodules
//!
//! - [`global`] — parsing of the `"global"` section
//! - [`resources`] — resource table management and parsing
//! - [`tasks`] — task, phase, and event parsing with prefix matching

pub mod global;
pub mod resources;
pub mod tasks;

use std::io::Read;
use std::path::Path;

use thiserror::Error;

pub use global::GlobalConfig;
pub use resources::ResourceTable;
pub use tasks::TaskConfig;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors that can occur during configuration parsing.
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("invalid scheduling policy: {0:?}")]
    InvalidPolicy(String),

    #[error("invalid resource type: {0:?}")]
    InvalidResourceType(String),

    #[error("invalid section: expected object for {0:?}")]
    InvalidSection(&'static str),

    #[error("missing required section: {0:?}")]
    MissingSection(&'static str),

    #[error("no events found in phase (tasks must have at least one event)")]
    NoEvents,

    #[error("invalid event value for {0:?}: {1}")]
    InvalidEventValue(String, &'static str),

    #[error("invalid {0} value: {1} (must be 0..=1024)")]
    InvalidUtilClamp(&'static str, u32),
}

// ---------------------------------------------------------------------------
// Top-level parsed config
// ---------------------------------------------------------------------------

/// The fully parsed configuration ready for use by the runtime.
#[derive(Debug)]
pub struct RtAppConfig {
    /// Global application settings.
    pub global: GlobalConfig,
    /// Resource table (explicit + auto-created).
    pub resources: ResourceTable,
    /// Parsed task definitions.
    pub tasks: Vec<TaskConfig>,
}

// ---------------------------------------------------------------------------
// Strip C-style comments from JSON
// ---------------------------------------------------------------------------

/// Strip C-style block comments (`/* ... */`) from JSON text.
///
/// rt-app's JSON files use C-style comments which are not valid JSON.
/// We strip them before parsing. This handles:
/// - Single-line block comments: `/* comment */`
/// - Multi-line block comments
/// - Comments inside strings are NOT stripped (we track string state)
fn strip_comments(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.char_indices().peekable();
    let mut in_string = false;
    let mut escape_next = false;

    while let Some(&(_, ch)) = chars.peek() {
        if escape_next {
            result.push(ch);
            chars.next();
            escape_next = false;
            continue;
        }

        if in_string {
            if ch == '\\' {
                escape_next = true;
                result.push(ch);
                chars.next();
            } else if ch == '"' {
                in_string = false;
                result.push(ch);
                chars.next();
            } else {
                result.push(ch);
                chars.next();
            }
            continue;
        }

        if ch == '"' {
            in_string = true;
            result.push(ch);
            chars.next();
            continue;
        }

        if ch == '/' {
            chars.next();
            if let Some(&(_, next_ch)) = chars.peek() {
                if next_ch == '*' {
                    // Start of block comment — skip until */
                    chars.next(); // consume '*'
                    skip_block_comment(&mut chars);
                    // Replace the comment with a space to avoid token concatenation
                    result.push(' ');
                    continue;
                }
                // Not a comment, push the '/' and continue
                result.push('/');
                // Don't consume next_ch, let the loop handle it
            } else {
                result.push('/');
            }
            continue;
        }

        result.push(ch);
        chars.next();
    }

    result
}

fn skip_block_comment(chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>) {
    while let Some((_, ch)) = chars.next() {
        if ch == '*' {
            if let Some(&(_, '/')) = chars.peek() {
                chars.next(); // consume '/'
                return;
            }
        }
    }
}

/// Strip trailing commas before `}` or `]` (a common relaxation in rt-app JSON).
fn strip_trailing_commas(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut in_string = false;
    let mut escape_next = false;

    while let Some(ch) = chars.next() {
        if escape_next {
            result.push(ch);
            escape_next = false;
            continue;
        }

        if in_string {
            if ch == '\\' {
                escape_next = true;
            } else if ch == '"' {
                in_string = false;
            }
            result.push(ch);
            continue;
        }

        if ch == '"' {
            in_string = true;
            result.push(ch);
            continue;
        }

        if ch == ',' {
            // Look ahead (skipping whitespace) to see if next non-ws is } or ]
            let mut peeked_ws = String::new();
            let mut found_close = false;
            while let Some(&next) = chars.peek() {
                if next.is_whitespace() {
                    peeked_ws.push(next);
                    chars.next();
                } else {
                    found_close = next == '}' || next == ']';
                    break;
                }
            }
            if found_close {
                // Skip the comma, but emit the whitespace
                result.push_str(&peeked_ws);
            } else {
                result.push(',');
                result.push_str(&peeked_ws);
            }
            continue;
        }

        result.push(ch);
    }

    result
}

/// Pre-process JSON text: strip comments and trailing commas.
fn preprocess_json(input: &str) -> String {
    strip_trailing_commas(&strip_comments(input))
}

// ---------------------------------------------------------------------------
// Parsing entry points
// ---------------------------------------------------------------------------

/// Parse a JSON configuration from a string.
pub fn parse_config_str(input: &str) -> Result<RtAppConfig, ConfigError> {
    let clean = preprocess_json(input);
    let root: serde_json::Value = serde_json::from_str(&clean)?;
    parse_from_value(&root)
}

/// Parse a JSON configuration from a file path.
pub fn parse_config(path: &Path) -> Result<RtAppConfig, ConfigError> {
    let content = std::fs::read_to_string(path)?;
    parse_config_str(&content)
}

/// Parse a JSON configuration from stdin.
pub fn parse_config_stdin() -> Result<RtAppConfig, ConfigError> {
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf)?;
    parse_config_str(&buf)
}

fn parse_from_value(root: &serde_json::Value) -> Result<RtAppConfig, ConfigError> {
    let root_obj = root
        .as_object()
        .ok_or(ConfigError::InvalidSection("root"))?;

    // Parse global section (optional)
    let global_config = global::parse_global(root_obj.get("global"))?;

    // Parse resources section (optional)
    let mut resource_table = resources::parse_resources(root_obj.get("resources"))?;

    // Parse tasks section (required)
    let tasks_val = root_obj
        .get("tasks")
        .ok_or(ConfigError::MissingSection("tasks"))?;
    let tasks = tasks::parse_tasks(tasks_val, global_config.default_policy, &mut resource_table)?;

    Ok(RtAppConfig {
        global: global_config,
        resources: resource_table,
        tasks,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_path(rel: &str) -> String {
        format!("{}/{}", env!("CARGO_MANIFEST_DIR"), rel)
    }

    #[test]
    fn strip_block_comments() {
        let input = r#"{ /* comment */ "key": "value" }"#;
        let output = strip_comments(input);
        assert!(output.contains("\"key\""));
        assert!(!output.contains("comment"));
    }

    #[test]
    fn strip_multiline_comments() {
        let input = "{\n\t/*\n\t * multi-line\n\t */\n\t\"key\": 1\n}";
        let output = strip_comments(input);
        assert!(output.contains("\"key\""));
        assert!(!output.contains("multi-line"));
    }

    #[test]
    fn strip_comments_preserves_strings() {
        let input = r#"{"key": "value /* not a comment */"}"#;
        let output = strip_comments(input);
        assert!(output.contains("/* not a comment */"));
    }

    #[test]
    fn strip_trailing_commas_object() {
        let input = r#"{"a": 1, "b": 2,}"#;
        let output = strip_trailing_commas(input);
        assert_eq!(output, r#"{"a": 1, "b": 2}"#);
    }

    #[test]
    fn strip_trailing_commas_array() {
        let input = r#"[1, 2, 3,]"#;
        let output = strip_trailing_commas(input);
        assert_eq!(output, r#"[1, 2, 3]"#);
    }

    #[test]
    fn strip_trailing_commas_preserves_strings() {
        let input = r#"{"key": "a,}"}"#;
        let output = strip_trailing_commas(input);
        assert_eq!(output, r#"{"key": "a,}"}"#);
    }

    #[test]
    fn preprocess_combined() {
        let input = r#"{
            /* comment */
            "key": 1,
        }"#;
        let output = preprocess_json(input);
        // Should be valid JSON after preprocessing
        let _: serde_json::Value = serde_json::from_str(&output).unwrap();
    }

    #[test]
    fn parse_minimal_config() {
        let json = r#"{
            "tasks": {
                "thread0": {
                    "run": 1000,
                    "sleep": 1000
                }
            }
        }"#;
        let config = parse_config_str(json).unwrap();
        assert_eq!(config.tasks.len(), 1);
        assert_eq!(config.tasks[0].name, "thread0");
        // Global should have defaults
        assert_eq!(config.global.duration_secs, -1);
    }

    #[test]
    fn parse_missing_tasks_error() {
        let json = r#"{"global": {"duration": 5}}"#;
        let result = parse_config_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn parse_example1() {
        let content =
            std::fs::read_to_string(&fixture_path("tests/fixtures/tutorial/example1.json"))
                .unwrap();
        let config = parse_config_str(&content).unwrap();
        assert_eq!(config.tasks.len(), 1);
        assert_eq!(config.tasks[0].name, "thread0");
        assert_eq!(config.global.duration_secs, 2);
        assert!(config.global.gnuplot);
        assert_eq!(
            config.global.ftrace.0,
            crate::types::FtraceLevel::MAIN
                | crate::types::FtraceLevel::TASK
                | crate::types::FtraceLevel::LOOP
                | crate::types::FtraceLevel::EVENT
        );
    }

    #[test]
    fn parse_example2() {
        let content =
            std::fs::read_to_string(&fixture_path("tests/fixtures/tutorial/example2.json"))
                .unwrap();
        let config = parse_config_str(&content).unwrap();
        assert_eq!(config.tasks.len(), 1);
        assert_eq!(config.tasks[0].num_instances, 1);
        // Has timer event
        let events = &config.tasks[0].phases[0].events;
        assert_eq!(events.len(), 2); // run + timer
    }

    #[test]
    fn parse_example3_phases() {
        let content =
            std::fs::read_to_string(&fixture_path("tests/fixtures/tutorial/example3.json"))
                .unwrap();
        let config = parse_config_str(&content).unwrap();
        assert_eq!(config.tasks.len(), 1);
        assert_eq!(config.tasks[0].phases.len(), 2);
        assert_eq!(config.tasks[0].num_instances, 12);
    }

    #[test]
    fn parse_example4_suspend_resume() {
        let content =
            std::fs::read_to_string(&fixture_path("tests/fixtures/tutorial/example4.json"))
                .unwrap();
        let config = parse_config_str(&content).unwrap();
        assert_eq!(config.tasks.len(), 2);
        // thread0 has run, resume, suspend
        let events0 = &config.tasks[0].phases[0].events;
        assert_eq!(events0.len(), 3);
    }

    #[test]
    fn parse_example6_mem_io() {
        let content =
            std::fs::read_to_string(&fixture_path("tests/fixtures/tutorial/example6.json"))
                .unwrap();
        let config = parse_config_str(&content).unwrap();
        assert_eq!(config.tasks.len(), 1);
        let events = &config.tasks[0].phases[0].events;
        // run, mem, sleep, iorun
        assert_eq!(events.len(), 4);
        assert_eq!(config.global.mem_buffer_size, 1048576);
    }

    #[test]
    fn parse_example7_barriers() {
        let content =
            std::fs::read_to_string(&fixture_path("tests/fixtures/tutorial/example7.json"))
                .unwrap();
        let config = parse_config_str(&content).unwrap();
        assert_eq!(config.tasks.len(), 2);
        // task0 has runtime1, sleep1, barrier1, runtime2, barrier2, runtime3, sleep3, barrier3
        let events0 = &config.tasks[0].phases[0].events;
        let barrier_count = events0
            .iter()
            .filter(|e| e.event_type == crate::types::ResourceType::Barrier)
            .count();
        assert_eq!(barrier_count, 3);
    }

    #[test]
    fn parse_example8_cpus() {
        let content =
            std::fs::read_to_string(&fixture_path("tests/fixtures/tutorial/example8.json"))
                .unwrap();
        let config = parse_config_str(&content).unwrap();
        assert_eq!(config.tasks.len(), 1);
        assert_eq!(config.tasks[0].cpus, vec![2]);
        assert_eq!(config.tasks[0].phases.len(), 3);
        assert_eq!(config.tasks[0].phases[0].cpus, vec![0]);
        assert_eq!(config.tasks[0].phases[1].cpus, vec![1]);
        // phase3 inherits from task level (cpus: [2]) — but at parse time,
        // phase cpus is empty (inheritance happens at runtime)
        assert!(config.tasks[0].phases[2].cpus.is_empty());
    }

    #[test]
    fn parse_example9_fork() {
        let content =
            std::fs::read_to_string(&fixture_path("tests/fixtures/tutorial/example9.json"))
                .unwrap();
        let config = parse_config_str(&content).unwrap();
        assert_eq!(config.tasks.len(), 3);
        // thread2 has instance = 0
        let t2 = config.tasks.iter().find(|t| t.name == "thread2").unwrap();
        assert_eq!(t2.num_instances, 0);
        // thread3 has fork events
        let t3 = config.tasks.iter().find(|t| t.name == "thread3").unwrap();
        let fork_events: Vec<_> = t3
            .phases
            .iter()
            .flat_map(|p| &p.events)
            .filter(|e| e.event_type == crate::types::ResourceType::Fork)
            .collect();
        assert_eq!(fork_events.len(), 2);
    }

    #[test]
    fn parse_example10_taskgroup() {
        let content =
            std::fs::read_to_string(&fixture_path("tests/fixtures/tutorial/example10.json"))
                .unwrap();
        let config = parse_config_str(&content).unwrap();
        assert_eq!(config.tasks.len(), 1);
        assert_eq!(config.tasks[0].taskgroup.as_deref(), Some("/tg1"));
    }

    #[test]
    fn parse_example11_phase_taskgroups() {
        let content =
            std::fs::read_to_string(&fixture_path("tests/fixtures/tutorial/example11.json"))
                .unwrap();
        let config = parse_config_str(&content).unwrap();
        assert_eq!(config.tasks.len(), 1);
        assert_eq!(config.tasks[0].phases.len(), 3);
        assert_eq!(
            config.tasks[0].phases[0].taskgroup.as_deref(),
            Some("/tg1/tg11")
        );
        // phase1 has no taskgroup
        assert!(config.tasks[0].phases[1].taskgroup.is_none());
        // phase2 has "/" taskgroup
        assert_eq!(config.tasks[0].phases[2].taskgroup.as_deref(), Some("/"));
    }

    #[test]
    fn parse_config_with_resources() {
        let json = r#"{
            "resources": {
                "mutex1": {"type": "mutex"},
                "barrier1": {"type": "barrier"}
            },
            "tasks": {
                "t1": {
                    "lock": "mutex1",
                    "run": 1000,
                    "unlock": "mutex1"
                }
            }
        }"#;
        let config = parse_config_str(json).unwrap();
        assert!(config.resources.len() >= 2);
    }

    #[test]
    fn parse_config_full_global() {
        let json = r#"{
            "global": {
                "duration": 5,
                "calibration": "CPU2",
                "default_policy": "SCHED_OTHER",
                "pi_enabled": true,
                "lock_pages": false,
                "logdir": "/tmp",
                "log_basename": "test",
                "ftrace": "main,task",
                "gnuplot": true,
                "io_device": "/dev/zero",
                "mem_buffer_size": 2097152,
                "cumulative_slack": true,
                "log_size": 4
            },
            "tasks": {
                "t1": {
                    "run": 1000,
                    "sleep": 1000
                }
            }
        }"#;
        let config = parse_config_str(json).unwrap();
        assert_eq!(config.global.duration_secs, 5);
        assert!(config.global.pi_enabled);
        assert!(!config.global.lock_pages);
        assert!(config.global.gnuplot);
        assert!(config.global.cumulative_slack);
    }
}
