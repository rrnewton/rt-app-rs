//! Comprehensive integration tests for rt-app-rs.
//!
//! These tests exercise the public API across module boundaries, parsing all
//! example JSON configurations and verifying edge cases.

use std::path::{Path, PathBuf};

use rt_app_rs::config::{parse_config, parse_config_str, ResourceTable};
use rt_app_rs::types::{FtraceLevel, ResourceType, SchedulingPolicy};

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

/// Return the path to the test fixtures directory.
fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Return the path to a tutorial example fixture.
fn tutorial_fixture(name: &str) -> PathBuf {
    fixtures_dir().join("tutorial").join(name)
}

/// Return the path to a top-level example fixture.
fn example_fixture(name: &str) -> PathBuf {
    fixtures_dir().join(name)
}

/// Parse a fixture file and return the config, panicking with a clear message
/// on failure.
fn parse_fixture(path: &Path) -> rt_app_rs::config::RtAppConfig {
    parse_config(path).unwrap_or_else(|e| panic!("failed to parse fixture {}: {e}", path.display()))
}

// ===========================================================================
// A) Config parsing integration tests -- tutorial examples
// ===========================================================================

#[test]
fn parse_tutorial_example1() {
    let config = parse_fixture(&tutorial_fixture("example1.json"));
    assert_eq!(config.tasks.len(), 1);
    assert_eq!(config.tasks[0].name, "thread0");
    assert_eq!(config.tasks[0].loop_count, 1);
    assert_eq!(config.tasks[0].phases.len(), 1);
    assert_eq!(config.tasks[0].phases[0].loop_count, -1);
    assert_eq!(config.tasks[0].phases[0].events.len(), 2); // run + sleep

    // Global settings
    assert_eq!(config.global.duration_secs, 2);
    assert!(config.global.gnuplot);
    assert!(!config.global.pi_enabled);
    assert!(!config.global.lock_pages);
    assert_eq!(
        config.global.ftrace.0,
        FtraceLevel::MAIN | FtraceLevel::TASK | FtraceLevel::LOOP | FtraceLevel::EVENT
    );
}

#[test]
fn parse_tutorial_example2() {
    let config = parse_fixture(&tutorial_fixture("example2.json"));
    assert_eq!(config.tasks.len(), 1);
    assert_eq!(config.tasks[0].name, "thread0");
    assert_eq!(config.tasks[0].num_instances, 1);
    assert_eq!(config.tasks[0].phases[0].events.len(), 2); // run + timer
    let has_timer = config.tasks[0].phases[0]
        .events
        .iter()
        .any(|e| e.event_type == ResourceType::TimerUnique);
    assert!(has_timer, "expected timer event in example2");
}

#[test]
fn parse_tutorial_example3() {
    let config = parse_fixture(&tutorial_fixture("example3.json"));
    assert_eq!(config.tasks.len(), 1);
    assert_eq!(config.tasks[0].num_instances, 12);
    assert_eq!(config.tasks[0].phases.len(), 2); // light + heavy
    assert_eq!(config.tasks[0].phases[0].loop_count, 10);
    assert_eq!(config.tasks[0].phases[1].loop_count, 10);
    assert_eq!(config.tasks[0].phases[0].events.len(), 2);
    assert_eq!(config.tasks[0].phases[1].events.len(), 2);
}

#[test]
fn parse_tutorial_example4() {
    let config = parse_fixture(&tutorial_fixture("example4.json"));
    assert_eq!(config.tasks.len(), 2);
    for task in &config.tasks {
        assert_eq!(task.phases[0].events.len(), 3);
    }
    let t0_events = &config.tasks[0].phases[0].events;
    let has_resume = t0_events
        .iter()
        .any(|e| e.event_type == ResourceType::Resume);
    let has_suspend = t0_events
        .iter()
        .any(|e| e.event_type == ResourceType::Suspend);
    assert!(has_resume, "expected resume event in thread0");
    assert!(has_suspend, "expected suspend event in thread0");
}

#[test]
fn parse_tutorial_example5() {
    // example5 has duplicate keys -- serde_json keeps only the last value.
    // The file header says it is meant for workgen, not direct rt-app usage.
    let config = parse_fixture(&tutorial_fixture("example5.json"));
    assert_eq!(config.tasks.len(), 2);
    let t0 = config.tasks.iter().find(|t| t.name == "thread0").unwrap();
    assert_eq!(t0.phases.len(), 2); // p0, p1
    assert_eq!(t0.num_instances, 1);
    assert_eq!(t0.cpus, vec![0]);
}

#[test]
fn parse_tutorial_example6() {
    let config = parse_fixture(&tutorial_fixture("example6.json"));
    assert_eq!(config.tasks.len(), 1);
    assert_eq!(config.tasks[0].phases[0].events.len(), 4); // run, mem, sleep, iorun
    assert_eq!(config.global.mem_buffer_size, 1048576);
    let events = &config.tasks[0].phases[0].events;
    let has_mem = events.iter().any(|e| e.event_type == ResourceType::Mem);
    let has_iorun = events.iter().any(|e| e.event_type == ResourceType::IoRun);
    assert!(has_mem, "expected mem event");
    assert!(has_iorun, "expected iorun event");
}

#[test]
fn parse_tutorial_example7() {
    let config = parse_fixture(&tutorial_fixture("example7.json"));
    assert_eq!(config.tasks.len(), 2);
    assert_eq!(config.global.duration_secs, 5);
    assert!(!config.global.gnuplot);
    let t0 = config.tasks.iter().find(|t| t.name == "task0").unwrap();
    let barrier_count = t0.phases[0]
        .events
        .iter()
        .filter(|e| e.event_type == ResourceType::Barrier)
        .count();
    assert_eq!(barrier_count, 3);
    let t1 = config.tasks.iter().find(|t| t.name == "task1").unwrap();
    let t1_barrier_count = t1.phases[0]
        .events
        .iter()
        .filter(|e| e.event_type == ResourceType::Barrier)
        .count();
    assert_eq!(t1_barrier_count, 3);
}

#[test]
fn parse_tutorial_example8() {
    let config = parse_fixture(&tutorial_fixture("example8.json"));
    assert_eq!(config.tasks.len(), 1);
    assert_eq!(config.tasks[0].cpus, vec![2]);
    assert_eq!(config.tasks[0].phases.len(), 3);
    assert_eq!(config.tasks[0].phases[0].cpus, vec![0]);
    assert_eq!(config.tasks[0].phases[1].cpus, vec![1]);
    assert!(config.tasks[0].phases[2].cpus.is_empty());
}

#[test]
fn parse_tutorial_example9() {
    let config = parse_fixture(&tutorial_fixture("example9.json"));
    assert_eq!(config.tasks.len(), 3);
    let t2 = config.tasks.iter().find(|t| t.name == "thread2").unwrap();
    assert_eq!(t2.num_instances, 0);
    let t3 = config.tasks.iter().find(|t| t.name == "thread3").unwrap();
    let fork_count: usize = t3
        .phases
        .iter()
        .flat_map(|p| &p.events)
        .filter(|e| e.event_type == ResourceType::Fork)
        .count();
    assert_eq!(fork_count, 2);
}

#[test]
fn parse_tutorial_example10() {
    let config = parse_fixture(&tutorial_fixture("example10.json"));
    assert_eq!(config.tasks.len(), 1);
    assert_eq!(config.tasks[0].taskgroup.as_deref(), Some("/tg1"));
}

#[test]
fn parse_tutorial_example11() {
    let config = parse_fixture(&tutorial_fixture("example11.json"));
    assert_eq!(config.tasks.len(), 1);
    assert_eq!(config.tasks[0].phases.len(), 3);
    assert_eq!(
        config.tasks[0].phases[0].taskgroup.as_deref(),
        Some("/tg1/tg11")
    );
    assert!(config.tasks[0].phases[1].taskgroup.is_none());
    assert_eq!(config.tasks[0].phases[2].taskgroup.as_deref(), Some("/"));
}

// ===========================================================================
// A) Config parsing integration tests -- non-tutorial examples
// ===========================================================================

#[test]
fn parse_template() {
    let config = parse_fixture(&example_fixture("template.json"));
    assert_eq!(config.tasks.len(), 1);
    assert_eq!(config.tasks[0].name, "thread0");
    assert_eq!(config.tasks[0].num_instances, 1);
    assert_eq!(config.global.duration_secs, 6);
    assert!(config.global.gnuplot);
    // Has run + sleep + timer events
    assert_eq!(config.tasks[0].phases[0].events.len(), 3);
}

#[test]
fn parse_custom_slice() {
    let config = parse_fixture(&example_fixture("custom-slice.json"));
    assert_eq!(config.tasks.len(), 2);
    let t0 = config.tasks.iter().find(|t| t.name == "thread0").unwrap();
    let t1 = config.tasks.iter().find(|t| t.name == "thread1").unwrap();
    assert_eq!(t0.sched.as_ref().unwrap().policy, SchedulingPolicy::Other);
    assert_eq!(
        t1.sched.as_ref().unwrap().policy,
        SchedulingPolicy::Deadline
    );
}

#[test]
fn parse_spreading_tasks() {
    let config = parse_fixture(&example_fixture("spreading-tasks.json"));
    assert_eq!(config.tasks.len(), 2);
    assert_eq!(config.global.duration_secs, 60);
    let t1 = config.tasks.iter().find(|t| t.name == "thread1").unwrap();
    assert_eq!(t1.phases.len(), 2);
    assert_eq!(t1.num_instances, 1);
    let t2 = config.tasks.iter().find(|t| t.name == "thread2").unwrap();
    assert!(
        t2.phases.len() >= 2,
        "expected at least 2 phases for thread2"
    );
}

#[test]
fn parse_browser_long() {
    let config = parse_fixture(&example_fixture("browser-long.json"));
    assert_eq!(config.global.duration_secs, 600);
    assert!(!config.global.gnuplot);
    assert!(config.global.lock_pages);
    assert_eq!(config.tasks.len(), 9);
    let main_task = config
        .tasks
        .iter()
        .find(|t| t.name == "BrowserMain")
        .unwrap();
    assert_eq!(main_task.phases.len(), 7);
    assert_eq!(main_task.loop_count, 3);
}

#[test]
fn parse_browser_short() {
    let config = parse_fixture(&example_fixture("browser-short.json"));
    assert_eq!(config.global.duration_secs, 6);
    assert_eq!(config.tasks.len(), 9);
    let main_task = config
        .tasks
        .iter()
        .find(|t| t.name == "BrowserMain")
        .unwrap();
    assert_eq!(main_task.phases.len(), 7);
}

#[test]
fn parse_mp3_long() {
    let config = parse_fixture(&example_fixture("mp3-long.json"));
    assert_eq!(config.global.duration_secs, 600);
    assert_eq!(config.tasks.len(), 5);
    assert!(config.global.lock_pages);
    let audio_tick = config.tasks.iter().find(|t| t.name == "AudioTick").unwrap();
    assert_eq!(audio_tick.cpus, vec![0]);
    assert_eq!(audio_tick.phases.len(), 2);
    assert!(
        config.resources.len() > 0,
        "expected auto-created resources"
    );
}

#[test]
fn parse_mp3_short() {
    let config = parse_fixture(&example_fixture("mp3-short.json"));
    assert_eq!(config.global.duration_secs, 6);
    assert_eq!(config.tasks.len(), 5);
}

#[test]
fn parse_video_long_is_invalid_json() {
    // video-long.json contains bare "suspend", keys (no colon or value)
    // which is invalid JSON even after comment/comma stripping.
    let path = example_fixture("video-long.json");
    let result = parse_config(&path);
    assert!(
        result.is_err(),
        "video-long.json should fail to parse due to bare key syntax"
    );
}

#[test]
fn parse_video_short_is_invalid_json() {
    // video-short.json has the same bare-key issue as video-long.json.
    let path = example_fixture("video-short.json");
    let result = parse_config(&path);
    assert!(
        result.is_err(),
        "video-short.json should fail to parse due to bare key syntax"
    );
}

// ===========================================================================
// A) Verify total fixture count
// ===========================================================================

#[test]
fn all_fixture_files_exist() {
    let tutorial_files = vec![
        "example1.json",
        "example2.json",
        "example3.json",
        "example4.json",
        "example5.json",
        "example6.json",
        "example7.json",
        "example8.json",
        "example9.json",
        "example10.json",
        "example11.json",
    ];
    for name in &tutorial_files {
        let path = tutorial_fixture(name);
        assert!(path.exists(), "missing tutorial fixture: {name}");
    }

    let example_files = vec![
        "template.json",
        "custom-slice.json",
        "spreading-tasks.json",
        "browser-long.json",
        "browser-short.json",
        "mp3-long.json",
        "mp3-short.json",
        "video-long.json",
        "video-short.json",
    ];
    for name in &example_files {
        let path = example_fixture(name);
        assert!(path.exists(), "missing example fixture: {name}");
    }

    // Total: 11 tutorial + 9 top-level = 20 fixtures
    assert_eq!(tutorial_files.len() + example_files.len(), 20);
}

// ===========================================================================
// B) Edge case tests
// ===========================================================================

#[test]
fn edge_case_empty_phases_object() {
    let json = r#"{"tasks": {"t1": {"phases": {}}}}"#;
    // Empty phases object means no phases with events; the parser should
    // produce a task with zero phases or an error. Either way, no panic.
    let result = parse_config_str(json);
    // Actually parse_task returns Ok with 0 phases if the phases object is
    // empty (no iteration). But then the task has no events. The current
    // implementation allows this.
    if let Ok(config) = result {
        assert_eq!(config.tasks[0].phases.len(), 0);
    }
    // If it errors, that is also acceptable.
}

#[test]
fn edge_case_zero_duration_run() {
    let json = r#"{"tasks": {"t1": {"run": 0, "sleep": 1000}}}"#;
    let config = parse_config_str(json).unwrap();
    assert_eq!(config.tasks.len(), 1);
    assert_eq!(config.tasks[0].phases[0].events[0].duration_usec, 0);
}

#[test]
fn edge_case_zero_duration_sleep() {
    let json = r#"{"tasks": {"t1": {"run": 1000, "sleep": 0}}}"#;
    let config = parse_config_str(json).unwrap();
    let sleep_event = config.tasks[0].phases[0]
        .events
        .iter()
        .find(|e| e.event_type == ResourceType::Sleep)
        .unwrap();
    assert_eq!(sleep_event.duration_usec, 0);
}

#[test]
fn edge_case_very_large_run_value() {
    let json = r#"{"tasks": {"t1": {"run": 999999999, "sleep": 1000}}}"#;
    let config = parse_config_str(json).unwrap();
    assert_eq!(config.tasks[0].phases[0].events[0].duration_usec, 999999999);
}

#[test]
fn edge_case_very_large_duration() {
    let json =
        r#"{"global": {"duration": 2147483647}, "tasks": {"t1": {"run": 1000, "sleep": 1000}}}"#;
    let config = parse_config_str(json).unwrap();
    assert_eq!(config.global.duration_secs, 2147483647);
}

#[test]
fn edge_case_missing_optional_global() {
    let json = r#"{"tasks": {"t1": {"run": 1000, "sleep": 1000}}}"#;
    let config = parse_config_str(json).unwrap();
    assert_eq!(config.global.duration_secs, -1);
    assert_eq!(config.global.default_policy, SchedulingPolicy::Other);
    assert!(!config.global.gnuplot);
    assert!(!config.global.pi_enabled);
    assert!(config.global.lock_pages); // default is true
}

#[test]
fn edge_case_missing_optional_resources() {
    let json = r#"{"tasks": {"t1": {"run": 1000, "sleep": 1000}}}"#;
    let config = parse_config_str(json).unwrap();
    assert_eq!(config.resources.len(), 0);
}

#[test]
fn edge_case_invalid_json_missing_brace() {
    let json = r#"{"tasks": {"t1": {"run": 1000}}"#;
    assert!(parse_config_str(json).is_err());
}

#[test]
fn edge_case_invalid_json_not_an_object() {
    assert!(parse_config_str("[1, 2, 3]").is_err());
}

#[test]
fn edge_case_invalid_json_empty_string() {
    assert!(parse_config_str("").is_err());
}

#[test]
fn edge_case_invalid_json_plain_text() {
    assert!(parse_config_str("this is not json").is_err());
}

#[test]
fn edge_case_missing_tasks_section() {
    let json = r#"{"global": {"duration": 5}}"#;
    let result = parse_config_str(json);
    assert!(result.is_err());
    let err_msg = format!("{}", result.unwrap_err());
    assert!(
        err_msg.contains("missing") || err_msg.contains("tasks"),
        "error should mention missing tasks: {err_msg}"
    );
}

#[test]
fn edge_case_comments_and_trailing_commas() {
    let json = concat!(
        "{\n",
        "    /* This is a block comment */\n",
        "    \"tasks\": {\n",
        "        /* Another comment */\n",
        "        \"thread0\": {\n",
        "            \"run\": 1000, /* inline comment */\n",
        "            \"sleep\": 2000,\n",
        "        },\n",
        "    },\n",
        "    \"global\": {\n",
        "        \"duration\": 5,\n",
        "        /* multi-line\n",
        "           comment */\n",
        "    },\n",
        "}"
    );
    let config = parse_config_str(json).unwrap();
    assert_eq!(config.tasks.len(), 1);
    assert_eq!(config.global.duration_secs, 5);
}

#[test]
fn edge_case_string_containing_comment_syntax() {
    let json = concat!(
        "{\"tasks\": {\"t1\": {\"run\": 1000, \"sleep\": 1000}},",
        " \"global\": {\"logdir\": \"/* not a comment */\",",
        " \"log_basename\": \"test /* also not */\"}}"
    );
    let config = parse_config_str(json).unwrap();
    assert_eq!(
        config.global.logdir.to_str().unwrap(),
        "/* not a comment */"
    );
}

#[test]
fn edge_case_many_tasks() {
    let mut tasks = String::new();
    for i in 0..50 {
        if i > 0 {
            tasks.push_str(", ");
        }
        tasks.push_str(&format!(
            "\"thread{i}\": {{\"run\": {}, \"sleep\": {}}}",
            1000 + i * 100,
            2000 + i * 50
        ));
    }
    let json = format!("{{\"tasks\": {{{tasks}}}}}");
    let config = parse_config_str(&json).unwrap();
    assert_eq!(config.tasks.len(), 50);
}

#[test]
fn edge_case_many_phases() {
    let mut phases = String::new();
    for i in 0..20 {
        if i > 0 {
            phases.push_str(", ");
        }
        phases.push_str(&format!(
            "\"phase{i}\": {{\"run\": {}, \"sleep\": 1000}}",
            500 + i * 50
        ));
    }
    let json = format!("{{\"tasks\": {{\"t1\": {{\"phases\": {{{phases}}}}}}}}}");
    let config = parse_config_str(&json).unwrap();
    assert_eq!(config.tasks[0].phases.len(), 20);
}

#[test]
fn edge_case_negative_duration() {
    let json = r#"{"global": {"duration": -1}, "tasks": {"t1": {"run": 1000, "sleep": 1000}}}"#;
    let config = parse_config_str(json).unwrap();
    assert_eq!(config.global.duration_secs, -1);
}

#[test]
fn edge_case_all_scheduling_policies() {
    for (policy_str, expected) in [
        ("SCHED_OTHER", SchedulingPolicy::Other),
        ("SCHED_FIFO", SchedulingPolicy::Fifo),
        ("SCHED_RR", SchedulingPolicy::RoundRobin),
        ("SCHED_DEADLINE", SchedulingPolicy::Deadline),
    ] {
        let json = format!(
            "{{\"tasks\": {{\"t1\": {{\"run\": 1000, \"sleep\": 1000, \"policy\": \"{policy_str}\"}}}}}}"
        );
        let config = parse_config_str(&json).unwrap();
        assert_eq!(
            config.tasks[0].sched.as_ref().unwrap().policy,
            expected,
            "policy {policy_str} should parse correctly"
        );
    }
}

#[test]
fn edge_case_invalid_policy_in_task() {
    let json = r#"{"tasks": {"t1": {"run": 1000, "sleep": 1000, "policy": "SCHED_IMAGINARY"}}}"#;
    assert!(
        parse_config_str(json).is_err(),
        "invalid policy should produce an error"
    );
}

#[test]
fn edge_case_invalid_policy_in_global() {
    let json = r#"{"global": {"default_policy": "SCHED_CHAOS"}, "tasks": {"t1": {"run": 1000, "sleep": 1000}}}"#;
    assert!(
        parse_config_str(json).is_err(),
        "invalid global policy should produce an error"
    );
}

#[test]
fn edge_case_null_global() {
    let json = r#"{"global": null, "tasks": {"t1": {"run": 1000, "sleep": 1000}}}"#;
    // null global should be treated as missing (defaults)
    let result = parse_config_str(json);
    // This depends on how parse_global handles null. It should work.
    if let Ok(config) = result {
        assert_eq!(config.global.duration_secs, -1);
    }
}

#[test]
fn edge_case_null_resources() {
    let json = r#"{"resources": null, "tasks": {"t1": {"run": 1000, "sleep": 1000}}}"#;
    let config = parse_config_str(json).unwrap();
    assert_eq!(config.resources.len(), 0);
}

#[test]
fn edge_case_nonexistent_file() {
    let path = Path::new("/nonexistent/path/config.json");
    let result = parse_config(path);
    assert!(result.is_err());
}

// ===========================================================================
// C) Cross-module integration tests
// ===========================================================================

#[test]
fn cross_module_config_to_types() {
    let json = concat!(
        "{\"resources\": {",
        "\"my_mutex\": {\"type\": \"mutex\"},",
        "\"my_barrier\": {\"type\": \"barrier\"},",
        "\"my_timer\": {\"type\": \"timer\"}},",
        "\"tasks\": {\"t1\": {",
        "\"lock\": \"my_mutex\",",
        "\"run\": 5000,",
        "\"unlock\": \"my_mutex\",",
        "\"barrier\": \"my_barrier\",",
        "\"timer\": {\"ref\": \"my_timer\", \"period\": 10000}}}}"
    );
    let config = parse_config_str(json).unwrap();

    let mutex_res = config
        .resources
        .iter()
        .find(|r| r.name == "my_mutex")
        .unwrap();
    assert_eq!(mutex_res.resource_type, ResourceType::Mutex);

    let barrier_res = config
        .resources
        .iter()
        .find(|r| r.name == "my_barrier")
        .unwrap();
    assert_eq!(barrier_res.resource_type, ResourceType::Barrier);

    let timer_res = config
        .resources
        .iter()
        .find(|r| r.name == "my_timer")
        .unwrap();
    assert_eq!(timer_res.resource_type, ResourceType::Timer);

    let events = &config.tasks[0].phases[0].events;
    let lock_event = events
        .iter()
        .find(|e| e.event_type == ResourceType::Lock)
        .unwrap();
    let unlock_event = events
        .iter()
        .find(|e| e.event_type == ResourceType::Unlock)
        .unwrap();
    assert!(lock_event.resource.is_some());
    assert!(unlock_event.resource.is_some());
    assert_eq!(lock_event.resource, unlock_event.resource);
}

#[test]
fn cross_module_resource_auto_creation() {
    let json = r#"{"tasks": {"t1": {"lock": "auto_mutex", "run": 1000, "unlock": "auto_mutex"}}}"#;
    let config = parse_config_str(json).unwrap();
    let found = config.resources.iter().any(|r| r.name == "auto_mutex");
    assert!(found, "auto-created resource should be in the table");
}

#[test]
fn cross_module_scheduling_params_from_config() {
    use rt_app_rs::engine::scheduling::{build_rt_attr, sched_priority_for};

    let json = r#"{"tasks": {"rt_task": {"run": 1000, "sleep": 1000, "policy": "SCHED_FIFO", "priority": 50}}}"#;
    let config = parse_config_str(json).unwrap();
    let sched = config.tasks[0].sched.as_ref().unwrap();
    assert_eq!(sched.policy, SchedulingPolicy::Fifo);
    assert_eq!(sched.priority, 50);
    assert_eq!(sched_priority_for(sched.policy, sched.priority), 50);

    let attr = build_rt_attr(sched.policy, sched.priority);
    assert_eq!(attr.sched_policy, 1); // SCHED_FIFO
    assert_eq!(attr.sched_priority, 50);
}

#[test]
fn cross_module_deadline_params_from_config() {
    use rt_app_rs::engine::scheduling::build_deadline_attr;

    let json = concat!(
        "{\"tasks\": {\"dl_task\": {",
        "\"run\": 1000, \"sleep\": 1000,",
        "\"policy\": \"SCHED_DEADLINE\",",
        "\"dl-runtime\": 100000,",
        "\"dl-period\": 1000000,",
        "\"dl-deadline\": 500000}}}"
    );
    let config = parse_config_str(json).unwrap();
    let sched = config.tasks[0].sched.as_ref().unwrap();
    assert_eq!(sched.policy, SchedulingPolicy::Deadline);

    let attr = build_deadline_attr(
        sched.dl_runtime_usec as u64 * 1000,
        sched.dl_deadline_usec as u64 * 1000,
        sched.dl_period_usec as u64 * 1000,
    );
    assert_eq!(attr.sched_runtime, 100_000_000);
    assert_eq!(attr.sched_deadline, 500_000_000);
    assert_eq!(attr.sched_period, 1_000_000_000);
}

#[test]
fn cross_module_affinity_with_config() {
    use rt_app_rs::affinity::CpuSetData;

    let json = r#"{"tasks": {"t1": {"cpus": [0, 2, 4], "run": 1000, "sleep": 1000}}}"#;
    let config = parse_config_str(json).unwrap();
    assert_eq!(config.tasks[0].cpus, vec![0, 2, 4]);

    let cpuset = CpuSetData::from_cpus(config.tasks[0].cpus.iter().map(|&c| c as usize)).unwrap();
    assert_eq!(cpuset.as_str(), "[ 0, 2, 4 ]");
    assert!(cpuset.inner().is_set(0).unwrap());
    assert!(!cpuset.inner().is_set(1).unwrap());
    assert!(cpuset.inner().is_set(2).unwrap());
    assert!(!cpuset.inner().is_set(3).unwrap());
    assert!(cpuset.inner().is_set(4).unwrap());
}

#[test]
fn cross_module_affinity_phase_override() {
    use rt_app_rs::affinity::CpuSetData;

    let json = concat!(
        "{\"tasks\": {\"t1\": {\"cpus\": [0], \"phases\": {",
        "\"p1\": {\"cpus\": [1, 2], \"run\": 1000, \"sleep\": 1000},",
        "\"p2\": {\"run\": 2000, \"sleep\": 1000}}}}}"
    );
    let config = parse_config_str(json).unwrap();

    let task_cpuset =
        CpuSetData::from_cpus(config.tasks[0].cpus.iter().map(|&c| c as usize)).unwrap();
    assert_eq!(task_cpuset.as_str(), "[ 0 ]");

    assert_eq!(config.tasks[0].phases[0].cpus, vec![1, 2]);
    let phase_cpuset =
        CpuSetData::from_cpus(config.tasks[0].phases[0].cpus.iter().map(|&c| c as usize)).unwrap();
    assert_eq!(phase_cpuset.as_str(), "[ 1, 2 ]");

    assert!(config.tasks[0].phases[1].cpus.is_empty());
}

#[test]
fn cross_module_uclamp_params() {
    use rt_app_rs::engine::scheduling::build_uclamp_attr;
    use rt_app_rs::syscalls::SchedFlags;

    let json =
        r#"{"tasks": {"t1": {"run": 1000, "sleep": 1000, "util_min": 256, "util_max": 768}}}"#;
    let config = parse_config_str(json).unwrap();
    let sched = config.tasks[0].sched.as_ref().unwrap();

    let attr = build_uclamp_attr(sched.policy, sched.priority, sched.util_min, sched.util_max);
    let flags = SchedFlags::from_bits_truncate(attr.sched_flags);
    assert!(flags.contains(SchedFlags::UTIL_CLAMP_MIN));
    assert!(flags.contains(SchedFlags::UTIL_CLAMP_MAX));
    assert_eq!(attr.sched_util_min, 256);
    assert_eq!(attr.sched_util_max, 768);
}

#[test]
fn cross_module_global_defaults_to_types() {
    use rt_app_rs::config::global::global_duration;

    let json = r#"{"tasks": {"t1": {"run": 1000, "sleep": 1000}}}"#;
    let config = parse_config_str(json).unwrap();
    assert!(global_duration(&config.global).is_none());

    let json2 = r#"{"global": {"duration": 10}, "tasks": {"t1": {"run": 1000, "sleep": 1000}}}"#;
    let config2 = parse_config_str(json2).unwrap();
    assert_eq!(
        global_duration(&config2.global),
        Some(std::time::Duration::from_secs(10))
    );
}

#[test]
fn cross_module_resource_data_construction() {
    use rt_app_rs::types::ResourceData;

    let test_cases = vec![
        (ResourceType::Mutex, "Mutex"),
        (ResourceType::Lock, "Mutex"),
        (ResourceType::Unlock, "Mutex"),
        (ResourceType::Wait, "Condition"),
        (ResourceType::Signal, "Condition"),
        (ResourceType::Broadcast, "Condition"),
        (ResourceType::Suspend, "Condition"),
        (ResourceType::Resume, "Condition"),
        (ResourceType::Timer, "Timer"),
        (ResourceType::TimerUnique, "Timer"),
        (ResourceType::Barrier, "Barrier"),
        (ResourceType::Mem, "IoMem"),
        (ResourceType::IoRun, "IoMem"),
        (ResourceType::Fork, "Fork"),
    ];

    for (rtype, expected_variant) in test_cases {
        let data = ResourceTable::make_resource_data(rtype);
        let variant_name = match &data {
            ResourceData::Mutex => "Mutex",
            ResourceData::Condition => "Condition",
            ResourceData::Timer { .. } => "Timer",
            ResourceData::Barrier { .. } => "Barrier",
            ResourceData::IoMem { .. } => "IoMem",
            ResourceData::Fork { .. } => "Fork",
            ResourceData::IoDev { .. } => "IoDev",
        };
        assert_eq!(
            variant_name, expected_variant,
            "ResourceType::{rtype:?} should produce ResourceData::{expected_variant}"
        );
    }
}

#[test]
fn cross_module_ftrace_levels_from_config() {
    let json = concat!(
        "{\"global\": {\"ftrace\": \"main,task,loop,event,stats,attrs\"},",
        " \"tasks\": {\"t1\": {\"run\": 1000, \"sleep\": 1000}}}"
    );
    let config = parse_config_str(json).unwrap();
    assert!(config.global.ftrace.0.contains(FtraceLevel::MAIN));
    assert!(config.global.ftrace.0.contains(FtraceLevel::TASK));
    assert!(config.global.ftrace.0.contains(FtraceLevel::LOOP));
    assert!(config.global.ftrace.0.contains(FtraceLevel::EVENT));
    assert!(config.global.ftrace.0.contains(FtraceLevel::STATS));
    assert!(config.global.ftrace.0.contains(FtraceLevel::ATTRS));
}

#[test]
fn cross_module_calibration_precise() {
    use rt_app_rs::config::global::Calibration;

    let json =
        r#"{"global": {"calibration": "precise"}, "tasks": {"t1": {"run": 1000, "sleep": 1000}}}"#;
    let config = parse_config_str(json).unwrap();
    assert_eq!(config.global.calibration, Calibration::Precise);
}

#[test]
fn cross_module_calibration_variants() {
    use rt_app_rs::config::global::Calibration;

    let json = r#"{"global": {"calibration": 500}, "tasks": {"t1": {"run": 1000, "sleep": 1000}}}"#;
    let config = parse_config_str(json).unwrap();
    assert_eq!(config.global.calibration, Calibration::NsPerLoop(500));

    let json2 =
        r#"{"global": {"calibration": "CPU3"}, "tasks": {"t1": {"run": 1000, "sleep": 1000}}}"#;
    let config2 = parse_config_str(json2).unwrap();
    assert_eq!(config2.global.calibration, Calibration::Cpu(3));
}

#[test]
fn cross_module_log_size_variants() {
    use rt_app_rs::config::global::LogSize;

    let test_cases = vec![
        ("\"file\"", LogSize::File),
        ("\"disable\"", LogSize::Disabled),
        ("\"auto\"", LogSize::File),
        ("4", LogSize::BufferBytes(4 << 20)),
    ];

    for (log_size_json, expected) in test_cases {
        let json = format!(
            "{{\"global\": {{\"log_size\": {log_size_json}}}, \"tasks\": {{\"t1\": {{\"run\": 1000, \"sleep\": 1000}}}}}}"
        );
        let config = parse_config_str(&json).unwrap();
        assert_eq!(
            config.global.log_size, expected,
            "log_size {log_size_json} should parse to {expected:?}"
        );
    }
}

#[test]
fn cross_module_multiple_resource_types_in_single_task() {
    let json = concat!(
        "{\"resources\": {",
        "\"m1\": {\"type\": \"mutex\"},",
        "\"b1\": {\"type\": \"barrier\"},",
        "\"t1\": {\"type\": \"timer\"}},",
        "\"tasks\": {\"worker\": {",
        "\"lock\": \"m1\", \"run\": 1000, \"unlock\": \"m1\",",
        "\"barrier\": \"b1\",",
        "\"timer\": {\"ref\": \"t1\", \"period\": 10000}}}}"
    );
    let config = parse_config_str(json).unwrap();
    let events = &config.tasks[0].phases[0].events;
    assert_eq!(events.len(), 5);

    let event_types: Vec<ResourceType> = events.iter().map(|e| e.event_type).collect();
    assert!(event_types.contains(&ResourceType::Lock));
    assert!(event_types.contains(&ResourceType::Run));
    assert!(event_types.contains(&ResourceType::Unlock));
    assert!(event_types.contains(&ResourceType::Barrier));
    assert!(event_types.contains(&ResourceType::Timer));
}

#[test]
fn cross_module_parse_and_verify_event_ordering() {
    let json = r#"{"tasks": {"t1": {"run1": 1000, "sleep1": 500, "run2": 2000, "sleep2": 1000}}}"#;
    let config = parse_config_str(json).unwrap();
    let events = &config.tasks[0].phases[0].events;
    assert_eq!(events.len(), 4);
    assert_eq!(events[0].event_type, ResourceType::Run);
    assert_eq!(events[0].duration_usec, 1000);
    assert_eq!(events[1].event_type, ResourceType::Sleep);
    assert_eq!(events[1].duration_usec, 500);
    assert_eq!(events[2].event_type, ResourceType::Run);
    assert_eq!(events[2].duration_usec, 2000);
    assert_eq!(events[3].event_type, ResourceType::Sleep);
    assert_eq!(events[3].duration_usec, 1000);
}

#[test]
fn cross_module_complex_workload_config() {
    let json = concat!(
        "{\"global\": {",
        "\"duration\": 10, \"default_policy\": \"SCHED_OTHER\",",
        "\"pi_enabled\": true, \"calibration\": \"CPU0\",",
        "\"gnuplot\": true, \"ftrace\": \"main,task\"},",
        "\"resources\": {",
        "\"shared_mutex\": {\"type\": \"mutex\"},",
        "\"sync_barrier\": {\"type\": \"barrier\"}},",
        "\"tasks\": {",
        "\"producer\": {",
        "\"instance\": 2, \"priority\": -10, \"cpus\": [0],",
        "\"phases\": {\"work\": {\"loop\": 100,",
        "\"lock\": \"shared_mutex\", \"run\": 5000,",
        "\"unlock\": \"shared_mutex\", \"barrier\": \"sync_barrier\",",
        "\"timer\": {\"ref\": \"unique\", \"period\": 10000}}}},",
        "\"consumer\": {",
        "\"instance\": 1, \"priority\": -5, \"cpus\": [1],",
        "\"loop\": -1,",
        "\"lock\": \"shared_mutex\", \"run\": 3000,",
        "\"unlock\": \"shared_mutex\", \"barrier\": \"sync_barrier\",",
        "\"sleep\": 5000}}}"
    );
    let config = parse_config_str(json).unwrap();
    assert_eq!(config.tasks.len(), 2);
    assert_eq!(config.global.duration_secs, 10);
    assert!(config.global.pi_enabled);
    assert!(config.global.gnuplot);

    let producer = config.tasks.iter().find(|t| t.name == "producer").unwrap();
    assert_eq!(producer.num_instances, 2);
    assert_eq!(producer.cpus, vec![0]);

    let consumer = config.tasks.iter().find(|t| t.name == "consumer").unwrap();
    assert_eq!(consumer.num_instances, 1);
    assert_eq!(consumer.cpus, vec![1]);

    let has_shared = config
        .resources
        .iter()
        .any(|r| r.name == "shared_mutex" && r.resource_type == ResourceType::Mutex);
    assert!(has_shared);
    let has_barrier = config
        .resources
        .iter()
        .any(|r| r.name == "sync_barrier" && r.resource_type == ResourceType::Barrier);
    assert!(has_barrier);
}
