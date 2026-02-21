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

use std::collections::HashMap;
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

/// Deduplicate JSON object keys by appending incrementing numeric suffixes.
///
/// rt-app's `workgen` script supports duplicate keys in JSON objects (which
/// standard JSON forbids). For example:
///
/// ```json
/// {"run": 5000, "sleep": 5000, "run": 5000}
/// ```
///
/// becomes:
///
/// ```json
/// {"run": 5000, "sleep": 5000, "run1": 5000}
/// ```
///
/// Each nesting level maintains its own set of seen keys. When a duplicate is
/// found, a numeric suffix starting at 1 is appended, incrementing until a
/// unique key is found. This mirrors the algorithm in the C rt-app's
/// `doc/workgen` Python script (`check_unikid_json()`), but operates on
/// characters rather than lines for correctness with arbitrary formatting.
fn deduplicate_keys(input: &str) -> String {
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut result = String::with_capacity(len);
    let mut pos = 0;

    // Stack of seen-key sets, one per `{` nesting level.
    // The outermost level is pushed when we encounter the first `{`.
    let mut key_stack: Vec<HashMap<String, usize>> = Vec::new();

    while pos < len {
        let b = bytes[pos];

        if b == b'"' {
            // Start of a JSON string — extract it and check if it's an object key.
            let (raw_str, content, end) = extract_json_string(bytes, pos);
            // Look ahead past whitespace after the closing quote to see if ':' follows.
            let colon_pos = skip_ascii_whitespace(bytes, end);
            let is_key = colon_pos < len && bytes[colon_pos] == b':';

            if is_key {
                if let Some(seen) = key_stack.last_mut() {
                    let count = seen.entry(content.clone()).or_insert(0);
                    *count += 1;
                    if *count > 1 {
                        // Duplicate key — find a unique suffixed version.
                        let new_key = find_unique_key(seen, &content);
                        result.push('"');
                        result.push_str(&new_key);
                        result.push('"');
                    } else {
                        result.push_str(raw_str);
                    }
                } else {
                    result.push_str(raw_str);
                }
            } else {
                // Not a key, just a string value — emit as-is.
                result.push_str(raw_str);
            }
            pos = end;
            continue;
        }

        if b == b'{' {
            key_stack.push(HashMap::new());
            result.push('{');
            pos += 1;
            continue;
        }

        if b == b'}' {
            key_stack.pop();
            result.push('}');
            pos += 1;
            continue;
        }

        result.push(b as char);
        pos += 1;
    }

    result
}

/// Extract a JSON string starting at `pos` (which must point to the opening `"`).
///
/// Returns `(raw_slice, raw_key_content, end_pos)` where `raw_key_content` is
/// the text between the quotes (preserving escape sequences) and `end_pos` is
/// the index just past the closing `"`.
fn extract_json_string(bytes: &[u8], start: usize) -> (&str, String, usize) {
    debug_assert_eq!(bytes[start], b'"');
    let mut pos = start + 1;

    while pos < bytes.len() {
        let b = bytes[pos];
        if b == b'\\' && pos + 1 < bytes.len() {
            pos += 2;
            continue;
        }
        if b == b'"' {
            let end = pos + 1;
            let raw = std::str::from_utf8(&bytes[start..end]).unwrap_or("");
            // Content between quotes, preserving escapes for faithful reproduction.
            let content = std::str::from_utf8(&bytes[start + 1..pos])
                .unwrap_or("")
                .to_owned();
            return (raw, content, end);
        }
        pos += 1;
    }

    // Unterminated string — return what we have.
    let raw = std::str::from_utf8(&bytes[start..pos]).unwrap_or("");
    let content = std::str::from_utf8(&bytes[start + 1..pos])
        .unwrap_or("")
        .to_owned();
    (raw, content, pos)
}

/// Skip ASCII whitespace starting at `pos`, returning the index of the first
/// non-whitespace byte (or `bytes.len()` if none).
fn skip_ascii_whitespace(bytes: &[u8], mut pos: usize) -> usize {
    while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
        pos += 1;
    }
    pos
}

/// Find a unique key name by appending incrementing suffixes (1, 2, 3, ...).
///
/// The newly chosen key is inserted into `seen` before returning so subsequent
/// duplicates of either the original or the suffixed name are handled correctly.
fn find_unique_key(seen: &mut HashMap<String, usize>, base: &str) -> String {
    let mut suffix = 1u32;
    loop {
        let candidate = format!("{base}{suffix}");
        if !seen.contains_key(&candidate) {
            seen.insert(candidate.clone(), 1);
            return candidate;
        }
        suffix += 1;
    }
}

/// Pre-process JSON text: strip comments, trailing commas, and deduplicate keys.
fn preprocess_json(input: &str) -> String {
    deduplicate_keys(&strip_trailing_commas(&strip_comments(input)))
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

    // -----------------------------------------------------------------------
    // Deduplicate keys tests
    // -----------------------------------------------------------------------

    #[test]
    fn dedup_simple_duplicate_keys() {
        let input = r#"{"run": 5000, "sleep": 5000, "run": 3000}"#;
        let output = deduplicate_keys(input);
        assert_eq!(output, r#"{"run": 5000, "sleep": 5000, "run1": 3000}"#);
    }

    #[test]
    fn dedup_triple_duplicate() {
        let input = r#"{"run": 1, "run": 2, "run": 3}"#;
        let output = deduplicate_keys(input);
        assert_eq!(output, r#"{"run": 1, "run1": 2, "run2": 3}"#);
    }

    #[test]
    fn dedup_nested_independent_levels() {
        // Each nesting level has its own key namespace.
        let input = r#"{"a": {"run": 1, "run": 2}, "b": {"run": 3, "run": 4}}"#;
        let output = deduplicate_keys(input);
        assert_eq!(
            output,
            r#"{"a": {"run": 1, "run1": 2}, "b": {"run": 3, "run1": 4}}"#
        );
    }

    #[test]
    fn dedup_no_false_positives_in_string_values() {
        // Keys inside string values should not be modified.
        let input = r#"{"key": "run", "run": 1}"#;
        let output = deduplicate_keys(input);
        assert_eq!(output, r#"{"key": "run", "run": 1}"#);
    }

    #[test]
    fn dedup_keys_with_existing_numeric_suffix() {
        // Keys like "run1" already present — should still deduplicate "run".
        let input = r#"{"run": 1, "run1": 2, "run": 3}"#;
        let output = deduplicate_keys(input);
        // "run" seen, "run1" seen, then second "run" needs suffix: try 1 (taken), try 2.
        assert_eq!(output, r#"{"run": 1, "run1": 2, "run2": 3}"#);
    }

    #[test]
    fn dedup_multiple_nesting_levels() {
        let input = r#"{
            "tasks": {
                "thread0": {
                    "run": 5000,
                    "sleep": 5000,
                    "run": 5000,
                    "sleep": 5000
                }
            }
        }"#;
        let output = deduplicate_keys(input);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        let thread0 = &parsed["tasks"]["thread0"];
        assert!(thread0.get("run").is_some());
        assert!(thread0.get("run1").is_some());
        assert!(thread0.get("sleep").is_some());
        assert!(thread0.get("sleep1").is_some());
    }

    #[test]
    fn dedup_preserves_no_dup_input() {
        let input = r#"{"a": 1, "b": 2, "c": 3}"#;
        let output = deduplicate_keys(input);
        assert_eq!(output, input);
    }

    #[test]
    fn dedup_empty_object() {
        let input = "{}";
        let output = deduplicate_keys(input);
        assert_eq!(output, "{}");
    }

    #[test]
    fn dedup_deeply_nested() {
        let input = r#"{"a": {"b": {"x": 1, "x": 2}}, "a": {"b": {"x": 3}}}"#;
        let output = deduplicate_keys(input);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        // Top level: "a" and "a1"
        assert!(parsed.get("a").is_some());
        assert!(parsed.get("a1").is_some());
        // Inner objects: "x" and "x1" in the first "a"
        let inner = &parsed["a"]["b"];
        assert!(inner.get("x").is_some());
        assert!(inner.get("x1").is_some());
    }

    #[test]
    fn dedup_escaped_quotes_in_keys() {
        // Key with escaped quote should be handled correctly.
        let input = r#"{"k\"ey": 1, "k\"ey": 2}"#;
        let output = deduplicate_keys(input);
        assert!(output.contains(r#""k\"ey1""#));
    }

    #[test]
    fn dedup_multiple_different_keys_duplicated() {
        let input =
            r#"{"lock": "m", "run": 100, "unlock": "m", "run": 200, "lock": "m", "unlock": "m"}"#;
        let output = deduplicate_keys(input);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert!(parsed.get("lock").is_some());
        assert!(parsed.get("lock1").is_some());
        assert!(parsed.get("run").is_some());
        assert!(parsed.get("run1").is_some());
        assert!(parsed.get("unlock").is_some());
        assert!(parsed.get("unlock1").is_some());
    }

    #[test]
    fn preprocess_deduplicates_after_comment_and_comma_strip() {
        let input = r#"{
            /* rt-app config */
            "run": 5000,
            "sleep": 5000,
            "run": 3000,
        }"#;
        let output = preprocess_json(input);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert!(parsed.get("run").is_some());
        assert!(parsed.get("run1").is_some());
        assert!(parsed.get("sleep").is_some());
    }
}
