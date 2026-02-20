//! Parsing of the `"tasks"` section: tasks, phases, and events.
//!
//! This is the most complex part of the JSON parser because:
//! - Events use prefix-matching on JSON keys (e.g. `"run1"`, `"run2"` are both "run" events)
//! - Tasks can use single-phase shorthand (events directly in the task object)
//!   or multi-phase (`"phases"` sub-object)
//! - Events auto-create resources when referenced by name

use serde_json::Value;

use crate::types::{ResourceIndex, ResourceType, SchedulingPolicy};

use super::resources::ResourceTable;
use super::ConfigError;

// ---------------------------------------------------------------------------
// Event prefixes and matching
// ---------------------------------------------------------------------------

/// All recognized event key prefixes, ordered for correct matching
/// (longer prefixes first where there could be ambiguity, e.g. "runtime"
/// before "run", "unlock" before "lock").
const EVENT_PREFIXES: &[(&str, EventKind)] = &[
    ("unlock", EventKind::Unlock),
    ("lock", EventKind::Lock),
    ("signal", EventKind::Signal),
    ("broad", EventKind::Broadcast),
    ("sync", EventKind::Sync),
    ("sleep", EventKind::Sleep),
    ("runtime", EventKind::Runtime),
    ("run", EventKind::Run),
    ("timer", EventKind::Timer),
    ("suspend", EventKind::Suspend),
    ("resume", EventKind::Resume),
    ("mem", EventKind::Mem),
    ("iorun", EventKind::IoRun),
    ("yield", EventKind::Yield),
    ("barrier", EventKind::Barrier),
    ("fork", EventKind::Fork),
    ("wait", EventKind::Wait),
];

/// Internal classification of an event from its JSON key prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EventKind {
    Run,
    Runtime,
    Sleep,
    Lock,
    Unlock,
    Signal,
    Broadcast,
    Wait,
    Sync,
    Timer,
    Suspend,
    Resume,
    Mem,
    IoRun,
    Yield,
    Barrier,
    Fork,
}

/// Try to match a JSON key to an event kind by prefix.
fn classify_event(key: &str) -> Option<EventKind> {
    EVENT_PREFIXES
        .iter()
        .find(|(prefix, _)| key.starts_with(prefix))
        .map(|&(_, kind)| kind)
}

/// Check if a JSON key is an event key.
pub fn is_event_key(key: &str) -> bool {
    classify_event(key).is_some()
}

// ---------------------------------------------------------------------------
// Parsed event
// ---------------------------------------------------------------------------

/// A parsed event from the JSON configuration.
#[derive(Debug, Clone)]
pub struct EventConfig {
    /// The original JSON key name (e.g. "run1", "lock_mutex").
    pub key: String,
    /// Human-readable name assigned during parsing.
    pub name: String,
    /// The event type as a `ResourceType`.
    pub event_type: ResourceType,
    /// Primary resource index, if applicable.
    pub resource: Option<ResourceIndex>,
    /// Dependent resource index, if applicable.
    pub dependency: Option<ResourceIndex>,
    /// Duration in microseconds for run/sleep/runtime events.
    pub duration_usec: i64,
    /// Byte count for mem/iorun events.
    pub count: u32,
}

// ---------------------------------------------------------------------------
// Parsed phase
// ---------------------------------------------------------------------------

/// A parsed execution phase.
#[derive(Debug, Clone)]
pub struct PhaseConfig {
    /// Number of iterations for this phase (1 = single run).
    pub loop_count: i32,
    /// Ordered events in this phase.
    pub events: Vec<EventConfig>,
    /// CPU affinity for this phase (empty = inherit).
    pub cpus: Vec<u32>,
    /// NUMA nodes for this phase.
    pub nodes_membind: Vec<u32>,
    /// Scheduling parameters override for this phase.
    pub sched: Option<SchedConfig>,
    /// Task group path for this phase.
    pub taskgroup: Option<String>,
}

// ---------------------------------------------------------------------------
// Parsed scheduling config
// ---------------------------------------------------------------------------

/// Scheduling parameters parsed from JSON.
#[derive(Debug, Clone)]
pub struct SchedConfig {
    pub policy: SchedulingPolicy,
    pub priority: i32,
    /// SCHED_DEADLINE runtime in microseconds.
    pub dl_runtime_usec: i64,
    /// SCHED_DEADLINE period in microseconds.
    pub dl_period_usec: i64,
    /// SCHED_DEADLINE deadline in microseconds.
    pub dl_deadline_usec: i64,
    /// Utilization clamp min (None = unchanged).
    pub util_min: Option<u32>,
    /// Utilization clamp max (None = unchanged).
    pub util_max: Option<u32>,
}

// ---------------------------------------------------------------------------
// Parsed task
// ---------------------------------------------------------------------------

/// A parsed task (thread template) from the JSON configuration.
#[derive(Debug, Clone)]
pub struct TaskConfig {
    /// Task name (the JSON key).
    pub name: String,
    /// Number of instances to create.
    pub num_instances: u32,
    /// CPU affinity at task level.
    pub cpus: Vec<u32>,
    /// NUMA nodes at task level.
    pub nodes_membind: Vec<u32>,
    /// Scheduling parameters at task level.
    pub sched: Option<SchedConfig>,
    /// Task group at task level.
    pub taskgroup: Option<String>,
    /// Initial delay in microseconds.
    pub delay_usec: i64,
    /// Number of outer loop iterations (-1 = infinite).
    pub loop_count: i32,
    /// Execution phases.
    pub phases: Vec<PhaseConfig>,
}

// ---------------------------------------------------------------------------
// Event parsing context
// ---------------------------------------------------------------------------

/// Context passed through event parsing to manage resource auto-creation.
pub struct ParseContext<'a> {
    pub resources: &'a mut ResourceTable,
    /// Unique tag for per-thread resources (mem buffers, unique timers).
    pub thread_tag: u64,
    /// Name of the current task (for suspend events).
    pub task_name: &'a str,
    /// Number of instances of the current task (for barrier counting).
    pub num_instances: u32,
}

// ---------------------------------------------------------------------------
// Event parsing
// ---------------------------------------------------------------------------

fn parse_event(
    key: &str,
    value: &Value,
    kind: EventKind,
    ctx: &mut ParseContext<'_>,
) -> Result<EventConfig, ConfigError> {
    match kind {
        EventKind::Run | EventKind::Runtime | EventKind::Sleep => {
            parse_duration_event(key, value, kind)
        }
        EventKind::Mem | EventKind::IoRun => parse_mem_event(key, value, kind, ctx),
        EventKind::Lock | EventKind::Unlock => parse_lock_event(key, value, kind, ctx),
        EventKind::Signal | EventKind::Broadcast => parse_signal_event(key, value, kind, ctx),
        EventKind::Wait | EventKind::Sync => parse_wait_event(key, value, kind, ctx),
        EventKind::Barrier => parse_barrier_event(key, value, ctx),
        EventKind::Timer => parse_timer_event(key, value, ctx),
        EventKind::Suspend => parse_suspend_event(key, ctx),
        EventKind::Resume => parse_resume_event(key, value, ctx),
        EventKind::Yield => Ok(parse_yield_event(key)),
        EventKind::Fork => parse_fork_event(key, value, ctx),
    }
}

fn parse_duration_event(
    key: &str,
    value: &Value,
    kind: EventKind,
) -> Result<EventConfig, ConfigError> {
    let duration = value
        .as_i64()
        .ok_or_else(|| ConfigError::InvalidEventValue(key.to_owned(), "expected integer"))?;

    let event_type = match kind {
        EventKind::Sleep => ResourceType::Sleep,
        EventKind::Runtime => ResourceType::Runtime,
        _ => ResourceType::Run,
    };

    Ok(EventConfig {
        key: key.to_owned(),
        name: key.to_owned(),
        event_type,
        resource: None,
        dependency: None,
        duration_usec: duration,
        count: 0,
    })
}

fn parse_mem_event(
    key: &str,
    value: &Value,
    kind: EventKind,
    ctx: &mut ParseContext<'_>,
) -> Result<EventConfig, ConfigError> {
    let count = value
        .as_i64()
        .ok_or_else(|| ConfigError::InvalidEventValue(key.to_owned(), "expected integer"))?;

    // Create a per-thread memory buffer resource
    let mem_name = format!("mem{:x}", ctx.thread_tag);
    let mem_idx = ctx.resources.get_or_create(&mem_name, ResourceType::Mem);

    let (event_type, dep) = if kind == EventKind::IoRun {
        let io_idx = ctx
            .resources
            .get_or_create("io_device", ResourceType::IoRun);
        (ResourceType::IoRun, Some(io_idx))
    } else {
        (ResourceType::Mem, None)
    };

    Ok(EventConfig {
        key: key.to_owned(),
        name: mem_name,
        event_type,
        resource: Some(mem_idx),
        dependency: dep,
        duration_usec: 0,
        count: count as u32,
    })
}

fn parse_lock_event(
    key: &str,
    value: &Value,
    kind: EventKind,
    ctx: &mut ParseContext<'_>,
) -> Result<EventConfig, ConfigError> {
    let ref_name = value
        .as_str()
        .ok_or_else(|| ConfigError::InvalidEventValue(key.to_owned(), "expected string"))?;

    let idx = ctx.resources.get_or_create(ref_name, ResourceType::Mutex);

    let event_type = if kind == EventKind::Lock {
        ResourceType::Lock
    } else {
        ResourceType::Unlock
    };

    Ok(EventConfig {
        key: key.to_owned(),
        name: format!("{key}:{ref_name}"),
        event_type,
        resource: Some(idx),
        dependency: None,
        duration_usec: 0,
        count: 0,
    })
}

fn parse_signal_event(
    key: &str,
    value: &Value,
    kind: EventKind,
    ctx: &mut ParseContext<'_>,
) -> Result<EventConfig, ConfigError> {
    let ref_name = value
        .as_str()
        .ok_or_else(|| ConfigError::InvalidEventValue(key.to_owned(), "expected string"))?;

    let idx = ctx.resources.get_or_create(ref_name, ResourceType::Wait);

    let event_type = if kind == EventKind::Signal {
        ResourceType::Signal
    } else {
        ResourceType::Broadcast
    };

    Ok(EventConfig {
        key: key.to_owned(),
        name: format!("{key}:{ref_name}"),
        event_type,
        resource: Some(idx),
        dependency: None,
        duration_usec: 0,
        count: 0,
    })
}

fn parse_wait_event(
    key: &str,
    value: &Value,
    kind: EventKind,
    ctx: &mut ParseContext<'_>,
) -> Result<EventConfig, ConfigError> {
    let obj = value
        .as_object()
        .ok_or_else(|| ConfigError::InvalidEventValue(key.to_owned(), "expected object"))?;

    let ref_name = obj.get("ref").and_then(|v| v.as_str()).unwrap_or("unknown");
    let mutex_name = obj
        .get("mutex")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let cond_idx = ctx.resources.get_or_create(ref_name, ResourceType::Wait);
    let mutex_idx = ctx.resources.get_or_create(mutex_name, ResourceType::Mutex);

    let event_type = if kind == EventKind::Wait {
        ResourceType::Wait
    } else {
        ResourceType::SigAndWait
    };

    Ok(EventConfig {
        key: key.to_owned(),
        name: format!("{key}:{ref_name}:{mutex_name}"),
        event_type,
        resource: Some(cond_idx),
        dependency: Some(mutex_idx),
        duration_usec: 0,
        count: 0,
    })
}

fn parse_barrier_event(
    key: &str,
    value: &Value,
    ctx: &mut ParseContext<'_>,
) -> Result<EventConfig, ConfigError> {
    let ref_name = value
        .as_str()
        .ok_or_else(|| ConfigError::InvalidEventValue(key.to_owned(), "expected string"))?;

    let idx = ctx.resources.get_or_create(ref_name, ResourceType::Barrier);

    Ok(EventConfig {
        key: key.to_owned(),
        name: format!("{key}:{ref_name}"),
        event_type: ResourceType::Barrier,
        resource: Some(idx),
        dependency: None,
        duration_usec: 0,
        count: ctx.num_instances,
    })
}

fn parse_timer_event(
    key: &str,
    value: &Value,
    ctx: &mut ParseContext<'_>,
) -> Result<EventConfig, ConfigError> {
    let obj = value
        .as_object()
        .ok_or_else(|| ConfigError::InvalidEventValue(key.to_owned(), "expected object"))?;

    let ref_name = obj.get("ref").and_then(|v| v.as_str()).unwrap_or("unknown");

    let (event_type, effective_name) = if ref_name.starts_with("unique") {
        let unique_name = format!("{ref_name}{:x}", ctx.thread_tag);
        (ResourceType::TimerUnique, unique_name)
    } else {
        (ResourceType::Timer, ref_name.to_owned())
    };

    let idx = ctx.resources.get_or_create(&effective_name, event_type);

    let period = obj.get("period").and_then(|v| v.as_i64()).unwrap_or(0);

    // Mode: "relative" (default) or "absolute"
    let _mode = obj
        .get("mode")
        .and_then(|v| v.as_str())
        .unwrap_or("relative");

    Ok(EventConfig {
        key: key.to_owned(),
        name: format!("{key}:{effective_name}"),
        event_type,
        resource: Some(idx),
        dependency: None,
        duration_usec: period,
        count: 0,
    })
}

fn parse_suspend_event(key: &str, ctx: &mut ParseContext<'_>) -> Result<EventConfig, ConfigError> {
    // Suspend creates cond+mutex resources named after the current task
    let cond_idx = ctx
        .resources
        .get_or_create(ctx.task_name, ResourceType::Wait);
    let mutex_idx = ctx
        .resources
        .get_or_create(ctx.task_name, ResourceType::Mutex);

    Ok(EventConfig {
        key: key.to_owned(),
        name: format!("{key}:{}:{}", ctx.task_name, ctx.task_name),
        event_type: ResourceType::Suspend,
        resource: Some(cond_idx),
        dependency: Some(mutex_idx),
        duration_usec: 0,
        count: 0,
    })
}

fn parse_resume_event(
    key: &str,
    value: &Value,
    ctx: &mut ParseContext<'_>,
) -> Result<EventConfig, ConfigError> {
    let ref_name = value
        .as_str()
        .ok_or_else(|| ConfigError::InvalidEventValue(key.to_owned(), "expected string"))?;

    let cond_idx = ctx.resources.get_or_create(ref_name, ResourceType::Wait);
    let mutex_idx = ctx.resources.get_or_create(ref_name, ResourceType::Mutex);

    Ok(EventConfig {
        key: key.to_owned(),
        name: format!("{key}:{ref_name}:{ref_name}"),
        event_type: ResourceType::Resume,
        resource: Some(cond_idx),
        dependency: Some(mutex_idx),
        duration_usec: 0,
        count: 0,
    })
}

fn parse_yield_event(key: &str) -> EventConfig {
    EventConfig {
        key: key.to_owned(),
        name: key.to_owned(),
        event_type: ResourceType::Yield,
        resource: None,
        dependency: None,
        duration_usec: 0,
        count: 0,
    }
}

fn parse_fork_event(
    key: &str,
    value: &Value,
    ctx: &mut ParseContext<'_>,
) -> Result<EventConfig, ConfigError> {
    let ref_name = value
        .as_str()
        .ok_or_else(|| ConfigError::InvalidEventValue(key.to_owned(), "expected string"))?;

    let idx = ctx.resources.get_or_create(ref_name, ResourceType::Fork);

    Ok(EventConfig {
        key: key.to_owned(),
        name: format!("{key}:{ref_name}"),
        event_type: ResourceType::Fork,
        resource: Some(idx),
        dependency: None,
        duration_usec: 0,
        count: 0,
    })
}

// ---------------------------------------------------------------------------
// Scheduling config parsing
// ---------------------------------------------------------------------------

/// Parse scheduling parameters from a JSON object.
/// `default_policy` is used when `"policy"` is not specified.
fn parse_sched(
    obj: &serde_json::Map<String, Value>,
    default_policy: SchedulingPolicy,
    is_task_level: bool,
) -> Result<Option<SchedConfig>, ConfigError> {
    let policy_str = obj
        .get("policy")
        .and_then(|v| v.as_str())
        .unwrap_or(match default_policy {
            SchedulingPolicy::Other => "SCHED_OTHER",
            SchedulingPolicy::Idle => "SCHED_IDLE",
            SchedulingPolicy::RoundRobin => "SCHED_RR",
            SchedulingPolicy::Fifo => "SCHED_FIFO",
            SchedulingPolicy::Deadline => "SCHED_DEADLINE",
            SchedulingPolicy::Same => "SCHED_SAME",
        });

    let policy: SchedulingPolicy = policy_str
        .parse()
        .map_err(|_| ConfigError::InvalidPolicy(policy_str.to_owned()))?;

    let default_priority = default_priority_for(policy);
    let priority = get_int(obj, "priority")
        .map(|v| v as i32)
        .unwrap_or(default_priority);

    let mut dl_runtime = get_int(obj, "dl-runtime").unwrap_or(0);
    let mut dl_period = get_int(obj, "dl-period").unwrap_or(dl_runtime);
    let mut dl_deadline = get_int(obj, "dl-deadline").unwrap_or(dl_period);

    // Legacy grammar support at task level
    if is_task_level {
        if dl_runtime == 0 {
            dl_runtime = get_int(obj, "runtime").unwrap_or(0);
        }
        if dl_period == 0 || dl_period == dl_runtime {
            dl_period = get_int(obj, "period").unwrap_or(dl_runtime);
        }
        if dl_deadline == 0 || dl_deadline == dl_period {
            dl_deadline = get_int(obj, "deadline").unwrap_or(dl_period);
        }
    }

    let util_min =
        get_int(obj, "util_min").and_then(|v| if v == -1 { None } else { Some(v as u32) });
    let util_max =
        get_int(obj, "util_max").and_then(|v| if v == -1 { None } else { Some(v as u32) });

    // Validate utilization clamp values
    if let Some(v) = util_min {
        if v > 1024 {
            return Err(ConfigError::InvalidUtilClamp("util_min", v));
        }
    }
    if let Some(v) = util_max {
        if v > 1024 {
            return Err(ConfigError::InvalidUtilClamp("util_max", v));
        }
    }

    // At phase level with `Same` policy, only produce a SchedConfig if at
    // least one meaningful parameter was set.
    if !is_task_level && policy == SchedulingPolicy::Same {
        let has_meaningful = priority != crate::types::THREAD_PRIORITY_UNCHANGED
            || dl_runtime != 0
            || dl_period != 0
            || dl_deadline != 0
            || util_min.is_some()
            || util_max.is_some();
        if !has_meaningful {
            return Ok(None);
        }
    }

    Ok(Some(SchedConfig {
        policy,
        priority,
        dl_runtime_usec: dl_runtime,
        dl_period_usec: dl_period,
        dl_deadline_usec: dl_deadline,
        util_min,
        util_max,
    }))
}

fn default_priority_for(policy: SchedulingPolicy) -> i32 {
    match policy {
        SchedulingPolicy::Same => crate::types::THREAD_PRIORITY_UNCHANGED,
        SchedulingPolicy::Other | SchedulingPolicy::Idle => crate::types::DEFAULT_THREAD_NICE,
        SchedulingPolicy::Fifo | SchedulingPolicy::RoundRobin => {
            crate::types::DEFAULT_THREAD_PRIORITY
        }
        SchedulingPolicy::Deadline => 0,
    }
}

/// Helper to extract an integer from a JSON object by key.
fn get_int(obj: &serde_json::Map<String, Value>, key: &str) -> Option<i64> {
    obj.get(key).and_then(|v| v.as_i64())
}

// ---------------------------------------------------------------------------
// CPU/NUMA parsing
// ---------------------------------------------------------------------------

fn parse_cpus(obj: &serde_json::Map<String, Value>) -> Vec<u32> {
    obj.get("cpus")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_u64().map(|n| n as u32))
                .collect()
        })
        .unwrap_or_default()
}

fn parse_nodes_membind(obj: &serde_json::Map<String, Value>) -> Vec<u32> {
    obj.get("nodes_membind")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_u64().map(|n| n as u32))
                .collect()
        })
        .unwrap_or_default()
}

fn parse_taskgroup(obj: &serde_json::Map<String, Value>) -> Option<String> {
    obj.get("taskgroup")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_owned())
}

// ---------------------------------------------------------------------------
// Phase parsing
// ---------------------------------------------------------------------------

fn parse_phase(
    obj: &serde_json::Map<String, Value>,
    ctx: &mut ParseContext<'_>,
) -> Result<PhaseConfig, ConfigError> {
    let loop_count = get_int(obj, "loop").unwrap_or(1) as i32;

    // Collect events in insertion order
    let mut events = Vec::new();
    for (key, val) in obj {
        if let Some(kind) = classify_event(key) {
            events.push(parse_event(key, val, kind, ctx)?);
        }
    }

    if events.is_empty() {
        return Err(ConfigError::NoEvents);
    }

    let cpus = parse_cpus(obj);
    let nodes_membind = parse_nodes_membind(obj);
    let sched = parse_sched(obj, SchedulingPolicy::Same, false)?;
    let taskgroup = parse_taskgroup(obj);

    Ok(PhaseConfig {
        loop_count,
        events,
        cpus,
        nodes_membind,
        sched,
        taskgroup,
    })
}

// ---------------------------------------------------------------------------
// Task parsing
// ---------------------------------------------------------------------------

fn parse_task(
    name: &str,
    obj: &serde_json::Map<String, Value>,
    default_policy: SchedulingPolicy,
    resources: &mut ResourceTable,
    task_index: usize,
) -> Result<TaskConfig, ConfigError> {
    let num_instances = get_int(obj, "instance").unwrap_or(1) as u32;
    let delay_usec = get_int(obj, "delay").unwrap_or(0);
    let cpus = parse_cpus(obj);
    let nodes_membind = parse_nodes_membind(obj);
    let sched = parse_sched(obj, default_policy, true)?;
    let taskgroup = parse_taskgroup(obj);

    // Use task_index as a unique tag for per-thread resources
    let thread_tag = task_index as u64;

    let mut ctx = ParseContext {
        resources,
        thread_tag,
        task_name: name,
        num_instances,
    };

    let (phases, loop_count) = if let Some(phases_val) = obj.get("phases") {
        // Multi-phase format
        let phases_obj = phases_val
            .as_object()
            .ok_or(ConfigError::InvalidSection("phases"))?;

        let mut phases = Vec::with_capacity(phases_obj.len());
        for (_phase_name, phase_val) in phases_obj {
            let phase_obj = phase_val
                .as_object()
                .ok_or(ConfigError::InvalidSection("phase"))?;
            phases.push(parse_phase(phase_obj, &mut ctx)?);
        }

        let loop_count = get_int(obj, "loop").unwrap_or(-1) as i32;
        (phases, loop_count)
    } else {
        // Single-phase shorthand: events are directly in the task object
        let phase = parse_phase(obj, &mut ctx)?;

        // Determine loop count: if "loop" was specified, use 1 for the outer
        // loop (the phase loop already has it). If not specified, loop forever.
        let has_explicit_loop = obj.contains_key("loop");
        let outer_loop = if has_explicit_loop { 1 } else { -1 };

        // In single-phase mode, the sched_data from the phase is redundant
        // with the task-level sched_data, so we clear it.
        let mut phase = phase;
        phase.sched = None;

        (vec![phase], outer_loop)
    };

    Ok(TaskConfig {
        name: name.to_owned(),
        num_instances,
        cpus,
        nodes_membind,
        sched,
        taskgroup,
        delay_usec,
        loop_count,
        phases,
    })
}

// ---------------------------------------------------------------------------
// Top-level tasks section parsing
// ---------------------------------------------------------------------------

/// Parse the `"tasks"` section from a JSON value.
///
/// Returns a list of `TaskConfig` and the (mutated) `ResourceTable` that
/// includes any auto-created resources.
pub fn parse_tasks(
    value: &serde_json::Value,
    default_policy: SchedulingPolicy,
    resources: &mut ResourceTable,
) -> Result<Vec<TaskConfig>, ConfigError> {
    let tasks_obj = value
        .as_object()
        .ok_or(ConfigError::InvalidSection("tasks"))?;

    let mut tasks = Vec::with_capacity(tasks_obj.len());
    for (idx, (name, val)) in tasks_obj.iter().enumerate() {
        let obj = val.as_object().ok_or(ConfigError::InvalidSection("task"))?;
        tasks.push(parse_task(name, obj, default_policy, resources, idx)?);
    }

    Ok(tasks)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_resources() -> ResourceTable {
        ResourceTable::new()
    }

    #[test]
    fn classify_run_events() {
        assert_eq!(classify_event("run"), Some(EventKind::Run));
        assert_eq!(classify_event("run1"), Some(EventKind::Run));
        assert_eq!(classify_event("run_foo"), Some(EventKind::Run));
        assert_eq!(classify_event("runtime"), Some(EventKind::Runtime));
        assert_eq!(classify_event("runtime1"), Some(EventKind::Runtime));
    }

    #[test]
    fn classify_lock_events() {
        assert_eq!(classify_event("lock"), Some(EventKind::Lock));
        assert_eq!(classify_event("lock_A"), Some(EventKind::Lock));
        assert_eq!(classify_event("unlock"), Some(EventKind::Unlock));
        assert_eq!(classify_event("unlock_B"), Some(EventKind::Unlock));
    }

    #[test]
    fn classify_non_event() {
        assert_eq!(classify_event("loop"), None);
        assert_eq!(classify_event("cpus"), None);
        assert_eq!(classify_event("policy"), None);
        assert_eq!(classify_event("instance"), None);
    }

    #[test]
    fn is_event_key_works() {
        assert!(is_event_key("run"));
        assert!(is_event_key("sleep1"));
        assert!(is_event_key("timer0"));
        assert!(is_event_key("barrier1"));
        assert!(!is_event_key("loop"));
        assert!(!is_event_key("cpus"));
    }

    #[test]
    fn parse_simple_task() {
        let json = serde_json::json!({
            "thread0": {
                "loop": -1,
                "run": 20000,
                "sleep": 80000
            }
        });
        let mut res = make_resources();
        let tasks = parse_tasks(&json, SchedulingPolicy::Other, &mut res).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].name, "thread0");
        // In single-phase mode with explicit "loop", the outer loop is 1
        // (the phase loop handles the repeating at -1).
        assert_eq!(tasks[0].loop_count, 1);
        assert_eq!(tasks[0].phases[0].loop_count, -1);
        assert_eq!(tasks[0].phases.len(), 1);
        assert_eq!(tasks[0].phases[0].events.len(), 2);
    }

    #[test]
    fn parse_task_with_phases() {
        let json = serde_json::json!({
            "thread0": {
                "loop": 1,
                "phases": {
                    "light": {
                        "loop": 10,
                        "run": 3000,
                        "timer": {"ref": "unique", "period": 30000}
                    },
                    "heavy": {
                        "loop": 10,
                        "run": 27000,
                        "timer": {"ref": "unique", "period": 30000}
                    }
                }
            }
        });
        let mut res = make_resources();
        let tasks = parse_tasks(&json, SchedulingPolicy::Other, &mut res).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].phases.len(), 2);
        assert_eq!(tasks[0].phases[0].loop_count, 10);
        assert_eq!(tasks[0].phases[0].events.len(), 2);
    }

    #[test]
    fn parse_lock_unlock_events() {
        let json = serde_json::json!({
            "t1": {
                "lock": "mutex1",
                "run": 1000,
                "unlock": "mutex1"
            }
        });
        let mut res = make_resources();
        let tasks = parse_tasks(&json, SchedulingPolicy::Other, &mut res).unwrap();
        let events = &tasks[0].phases[0].events;
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].event_type, ResourceType::Lock);
        assert_eq!(events[1].event_type, ResourceType::Run);
        assert_eq!(events[2].event_type, ResourceType::Unlock);
        // Lock and unlock should reference the same mutex
        assert_eq!(events[0].resource, events[2].resource);
    }

    #[test]
    fn parse_suspend_resume() {
        let json = serde_json::json!({
            "thread0": {
                "run": 10000,
                "resume": "thread1",
                "suspend": "thread0"
            }
        });
        let mut res = make_resources();
        let tasks = parse_tasks(&json, SchedulingPolicy::Other, &mut res).unwrap();
        let events = &tasks[0].phases[0].events;
        assert_eq!(events.len(), 3);
        assert_eq!(events[1].event_type, ResourceType::Resume);
        assert_eq!(events[2].event_type, ResourceType::Suspend);
    }

    #[test]
    fn parse_barrier_event() {
        let json = serde_json::json!({
            "task0": {
                "run": 1000,
                "barrier1": "FIRST"
            }
        });
        let mut res = make_resources();
        let tasks = parse_tasks(&json, SchedulingPolicy::Other, &mut res).unwrap();
        let events = &tasks[0].phases[0].events;
        assert_eq!(events[1].event_type, ResourceType::Barrier);
        assert!(events[1].resource.is_some());
    }

    #[test]
    fn parse_fork_event() {
        let json = serde_json::json!({
            "t1": {
                "fork": "thread2",
                "run": 1000
            }
        });
        let mut res = make_resources();
        let tasks = parse_tasks(&json, SchedulingPolicy::Other, &mut res).unwrap();
        let events = &tasks[0].phases[0].events;
        assert_eq!(events[0].event_type, ResourceType::Fork);
    }

    #[test]
    fn parse_wait_event() {
        let json = serde_json::json!({
            "t1": {
                "wait": {"ref": "queue", "mutex": "m1"},
                "run": 1000
            }
        });
        let mut res = make_resources();
        let tasks = parse_tasks(&json, SchedulingPolicy::Other, &mut res).unwrap();
        let events = &tasks[0].phases[0].events;
        assert_eq!(events[0].event_type, ResourceType::Wait);
        assert!(events[0].resource.is_some());
        assert!(events[0].dependency.is_some());
    }

    #[test]
    fn parse_mem_iorun() {
        let json = serde_json::json!({
            "t1": {
                "mem": 1000,
                "iorun": 2000,
                "run": 500
            }
        });
        let mut res = make_resources();
        let tasks = parse_tasks(&json, SchedulingPolicy::Other, &mut res).unwrap();
        let events = &tasks[0].phases[0].events;

        let mem_ev = events
            .iter()
            .find(|e| e.event_type == ResourceType::Mem)
            .unwrap();
        assert_eq!(mem_ev.count, 1000);

        let io_ev = events
            .iter()
            .find(|e| e.event_type == ResourceType::IoRun)
            .unwrap();
        assert_eq!(io_ev.count, 2000);
        assert!(io_ev.dependency.is_some()); // io_device dependency
    }

    #[test]
    fn parse_scheduling_params() {
        let json = serde_json::json!({
            "t1": {
                "policy": "SCHED_FIFO",
                "priority": 50,
                "run": 1000,
                "sleep": 1000
            }
        });
        let mut res = make_resources();
        let tasks = parse_tasks(&json, SchedulingPolicy::Other, &mut res).unwrap();
        let sched = tasks[0].sched.as_ref().unwrap();
        assert_eq!(sched.policy, SchedulingPolicy::Fifo);
        assert_eq!(sched.priority, 50);
    }

    #[test]
    fn parse_deadline_params() {
        let json = serde_json::json!({
            "t1": {
                "policy": "SCHED_DEADLINE",
                "dl-runtime": 1000,
                "dl-period": 5000,
                "dl-deadline": 4000,
                "run": 500,
                "sleep": 500
            }
        });
        let mut res = make_resources();
        let tasks = parse_tasks(&json, SchedulingPolicy::Other, &mut res).unwrap();
        let sched = tasks[0].sched.as_ref().unwrap();
        assert_eq!(sched.policy, SchedulingPolicy::Deadline);
        assert_eq!(sched.dl_runtime_usec, 1000);
        assert_eq!(sched.dl_period_usec, 5000);
        assert_eq!(sched.dl_deadline_usec, 4000);
    }

    #[test]
    fn parse_cpus_and_numa() {
        let json = serde_json::json!({
            "t1": {
                "cpus": [0, 2, 3],
                "nodes_membind": [1],
                "run": 1000,
                "sleep": 1000
            }
        });
        let mut res = make_resources();
        let tasks = parse_tasks(&json, SchedulingPolicy::Other, &mut res).unwrap();
        assert_eq!(tasks[0].cpus, vec![0, 2, 3]);
        assert_eq!(tasks[0].nodes_membind, vec![1]);
    }

    #[test]
    fn parse_instance_count() {
        let json = serde_json::json!({
            "t1": {
                "instance": 4,
                "run": 1000,
                "sleep": 1000
            }
        });
        let mut res = make_resources();
        let tasks = parse_tasks(&json, SchedulingPolicy::Other, &mut res).unwrap();
        assert_eq!(tasks[0].num_instances, 4);
    }

    #[test]
    fn auto_create_resources() {
        let json = serde_json::json!({
            "t1": {
                "lock": "my_mutex",
                "run": 1000,
                "unlock": "my_mutex"
            }
        });
        let mut res = make_resources();
        parse_tasks(&json, SchedulingPolicy::Other, &mut res).unwrap();
        // The mutex should have been auto-created
        assert!(res.len() > 0);
    }

    #[test]
    fn no_events_error() {
        let json = serde_json::json!({
            "t1": {
                "instance": 1
            }
        });
        let mut res = make_resources();
        let result = parse_tasks(&json, SchedulingPolicy::Other, &mut res);
        assert!(result.is_err());
    }

    #[test]
    fn util_clamp_validation() {
        let json = serde_json::json!({
            "t1": {
                "util_min": 2000,
                "run": 1000,
                "sleep": 1000
            }
        });
        let mut res = make_resources();
        let result = parse_tasks(&json, SchedulingPolicy::Other, &mut res);
        assert!(result.is_err());
    }

    #[test]
    fn parse_yield_event() {
        let json = serde_json::json!({
            "t1": {
                "run": 1000,
                "yield": ""
            }
        });
        let mut res = make_resources();
        let tasks = parse_tasks(&json, SchedulingPolicy::Other, &mut res).unwrap();
        let events = &tasks[0].phases[0].events;
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].event_type, ResourceType::Yield);
    }

    #[test]
    fn parse_timer_modes() {
        let json = serde_json::json!({
            "t1": {
                "run": 1000,
                "timer0": {"ref": "unique", "period": 20000, "mode": "absolute"},
                "timer1": {"ref": "tick", "period": 30000}
            }
        });
        let mut res = make_resources();
        let tasks = parse_tasks(&json, SchedulingPolicy::Other, &mut res).unwrap();
        let events = &tasks[0].phases[0].events;
        // Should have run + 2 timer events
        assert_eq!(events.len(), 3);
        // The "unique" timer should be TimerUnique
        let unique_timer = events
            .iter()
            .find(|e| e.event_type == ResourceType::TimerUnique)
            .unwrap();
        assert!(unique_timer.name.starts_with("timer0:unique"));
        // The "tick" timer should be Timer
        let shared_timer = events
            .iter()
            .find(|e| e.event_type == ResourceType::Timer)
            .unwrap();
        assert!(shared_timer.name.contains("tick"));
    }

    #[test]
    fn parse_signal_event() {
        let json = serde_json::json!({
            "t1": {
                "signal": "queue",
                "run": 1000
            }
        });
        let mut res = make_resources();
        let tasks = parse_tasks(&json, SchedulingPolicy::Other, &mut res).unwrap();
        let events = &tasks[0].phases[0].events;
        let sig_ev = events
            .iter()
            .find(|e| e.event_type == ResourceType::Signal)
            .unwrap();
        assert_eq!(sig_ev.name, "signal:queue");
    }

    #[test]
    fn parse_sync_event() {
        let json = serde_json::json!({
            "t1": {
                "sync": {"ref": "cond1", "mutex": "m1"},
                "run": 1000
            }
        });
        let mut res = make_resources();
        let tasks = parse_tasks(&json, SchedulingPolicy::Other, &mut res).unwrap();
        let events = &tasks[0].phases[0].events;
        let sync_ev = events
            .iter()
            .find(|e| e.event_type == ResourceType::SigAndWait)
            .unwrap();
        assert!(sync_ev.resource.is_some());
        assert!(sync_ev.dependency.is_some());
    }

    #[test]
    fn single_phase_loop_infinite() {
        // When no "loop" specified in single-phase mode, outer loop should be -1
        let json = serde_json::json!({
            "t1": {
                "run": 1000,
                "sleep": 1000
            }
        });
        let mut res = make_resources();
        let tasks = parse_tasks(&json, SchedulingPolicy::Other, &mut res).unwrap();
        assert_eq!(tasks[0].loop_count, -1);
    }

    #[test]
    fn single_phase_loop_explicit() {
        // When "loop" is specified in single-phase mode, outer loop should be 1
        let json = serde_json::json!({
            "t1": {
                "loop": 5,
                "run": 1000,
                "sleep": 1000
            }
        });
        let mut res = make_resources();
        let tasks = parse_tasks(&json, SchedulingPolicy::Other, &mut res).unwrap();
        assert_eq!(tasks[0].loop_count, 1);
        assert_eq!(tasks[0].phases[0].loop_count, 5);
    }

    #[test]
    fn taskgroup_parsing() {
        let json = serde_json::json!({
            "t1": {
                "taskgroup": "/tg1",
                "run": 1000,
                "sleep": 1000
            }
        });
        let mut res = make_resources();
        let tasks = parse_tasks(&json, SchedulingPolicy::Other, &mut res).unwrap();
        assert_eq!(tasks[0].taskgroup.as_deref(), Some("/tg1"));
    }

    #[test]
    fn phase_taskgroup() {
        let json = serde_json::json!({
            "t1": {
                "phases": {
                    "p1": {
                        "run": 1000,
                        "sleep": 1000,
                        "taskgroup": "/tg1/tg11"
                    }
                }
            }
        });
        let mut res = make_resources();
        let tasks = parse_tasks(&json, SchedulingPolicy::Other, &mut res).unwrap();
        assert_eq!(tasks[0].phases[0].taskgroup.as_deref(), Some("/tg1/tg11"));
    }

    #[test]
    fn delay_parsing() {
        let json = serde_json::json!({
            "t1": {
                "delay": 5000,
                "run": 1000,
                "sleep": 1000
            }
        });
        let mut res = make_resources();
        let tasks = parse_tasks(&json, SchedulingPolicy::Other, &mut res).unwrap();
        assert_eq!(tasks[0].delay_usec, 5000);
    }
}
