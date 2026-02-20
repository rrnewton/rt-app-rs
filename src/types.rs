//! Core types for rt-app-rs, ported from rt-app_types.h and dl_syscalls.h.
//!
//! All types use idiomatic Rust: enums replace C unions, `Option` replaces
//! sentinel values, `Duration` replaces raw microsecond/nanosecond integers,
//! and newtype wrappers provide type safety for indices and identifiers.

use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

use bitflags::bitflags;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default thread priority when none is specified.
pub const DEFAULT_THREAD_PRIORITY: i32 = 10;

/// Default thread nice value.
pub const DEFAULT_THREAD_NICE: i32 = 0;

/// Sentinel: leave the thread priority unchanged.
pub const THREAD_PRIORITY_UNCHANGED: i32 = i32::MAX;

// ---------------------------------------------------------------------------
// Exit codes
// ---------------------------------------------------------------------------

/// Application exit codes matching the C original.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ExitCode {
    Success = 0,
    Failure = 1,
    InvalidConfig = 2,
    InvalidCommandLine = 3,
}

impl From<ExitCode> for i32 {
    fn from(code: ExitCode) -> Self {
        code as i32
    }
}

// ---------------------------------------------------------------------------
// Newtype index wrappers
// ---------------------------------------------------------------------------

/// Zero-based index of a thread within the configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ThreadIndex(pub usize);

/// Zero-based index of a shared resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ResourceIndex(pub usize);

/// Zero-based index of a phase within a thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PhaseIndex(pub usize);

/// Zero-based index of an event within a phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EventIndex(pub usize);

impl fmt::Display for ThreadIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl fmt::Display for ResourceIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl fmt::Display for PhaseIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl fmt::Display for EventIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ---------------------------------------------------------------------------
// Scheduling policy
// ---------------------------------------------------------------------------

/// Linux scheduling policies. Values match the kernel constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SchedulingPolicy {
    /// SCHED_OTHER (CFS, the default).
    #[serde(rename = "SCHED_OTHER")]
    Other,
    /// SCHED_IDLE.
    #[serde(rename = "SCHED_IDLE")]
    Idle,
    /// SCHED_RR (round-robin real-time).
    #[serde(rename = "SCHED_RR")]
    RoundRobin,
    /// SCHED_FIFO (first-in-first-out real-time).
    #[serde(rename = "SCHED_FIFO")]
    Fifo,
    /// SCHED_DEADLINE (earliest deadline first).
    #[serde(rename = "SCHED_DEADLINE")]
    Deadline,
    /// Inherit the policy from the parent / keep the same.
    #[serde(rename = "SCHED_SAME")]
    Same,
}

impl SchedulingPolicy {
    /// Return the Linux kernel constant for this policy, or `None` for `Same`.
    pub fn to_kernel_id(self) -> Option<u32> {
        match self {
            Self::Other => Some(0),      // SCHED_OTHER
            Self::Idle => Some(5),       // SCHED_IDLE
            Self::RoundRobin => Some(2), // SCHED_RR
            Self::Fifo => Some(1),       // SCHED_FIFO
            Self::Deadline => Some(6),   // SCHED_DEADLINE
            Self::Same => None,
        }
    }
}

impl fmt::Display for SchedulingPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Other => "SCHED_OTHER",
            Self::Idle => "SCHED_IDLE",
            Self::RoundRobin => "SCHED_RR",
            Self::Fifo => "SCHED_FIFO",
            Self::Deadline => "SCHED_DEADLINE",
            Self::Same => "SCHED_SAME",
        };
        f.write_str(s)
    }
}

impl FromStr for SchedulingPolicy {
    type Err = ParseEnumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "SCHED_OTHER" => Ok(Self::Other),
            "SCHED_IDLE" => Ok(Self::Idle),
            "SCHED_RR" => Ok(Self::RoundRobin),
            "SCHED_FIFO" => Ok(Self::Fifo),
            "SCHED_DEADLINE" => Ok(Self::Deadline),
            "SCHED_SAME" => Ok(Self::Same),
            _ => Err(ParseEnumError {
                type_name: "SchedulingPolicy",
                value: s.to_owned(),
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// Resource type
// ---------------------------------------------------------------------------

/// The kind of operation an event performs on a resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResourceType {
    #[serde(rename = "unknown")]
    Unknown,
    #[serde(rename = "mutex")]
    Mutex,
    #[serde(rename = "wait")]
    Wait,
    #[serde(rename = "signal")]
    Signal,
    #[serde(rename = "broadcast")]
    Broadcast,
    #[serde(rename = "sleep")]
    Sleep,
    #[serde(rename = "run")]
    Run,
    #[serde(rename = "sync")]
    SigAndWait,
    #[serde(rename = "lock")]
    Lock,
    #[serde(rename = "unlock")]
    Unlock,
    #[serde(rename = "timer")]
    Timer,
    #[serde(rename = "timer_unique")]
    TimerUnique,
    #[serde(rename = "suspend")]
    Suspend,
    #[serde(rename = "resume")]
    Resume,
    #[serde(rename = "mem")]
    Mem,
    #[serde(rename = "iorun")]
    IoRun,
    #[serde(rename = "runtime")]
    Runtime,
    #[serde(rename = "yield")]
    Yield,
    #[serde(rename = "barrier")]
    Barrier,
    #[serde(rename = "fork")]
    Fork,
}

impl fmt::Display for ResourceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Unknown => "unknown",
            Self::Mutex => "mutex",
            Self::Wait => "wait",
            Self::Signal => "signal",
            Self::Broadcast => "broadcast",
            Self::Sleep => "sleep",
            Self::Run => "run",
            Self::SigAndWait => "sync",
            Self::Lock => "lock",
            Self::Unlock => "unlock",
            Self::Timer => "timer",
            Self::TimerUnique => "timer_unique",
            Self::Suspend => "suspend",
            Self::Resume => "resume",
            Self::Mem => "mem",
            Self::IoRun => "iorun",
            Self::Runtime => "runtime",
            Self::Yield => "yield",
            Self::Barrier => "barrier",
            Self::Fork => "fork",
        };
        f.write_str(s)
    }
}

impl FromStr for ResourceType {
    type Err = ParseEnumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "unknown" => Ok(Self::Unknown),
            "mutex" => Ok(Self::Mutex),
            "wait" => Ok(Self::Wait),
            "signal" => Ok(Self::Signal),
            "broadcast" => Ok(Self::Broadcast),
            "sleep" => Ok(Self::Sleep),
            "run" => Ok(Self::Run),
            "sync" => Ok(Self::SigAndWait),
            "lock" => Ok(Self::Lock),
            "unlock" => Ok(Self::Unlock),
            "timer" => Ok(Self::Timer),
            "timer_unique" => Ok(Self::TimerUnique),
            "suspend" => Ok(Self::Suspend),
            "resume" => Ok(Self::Resume),
            "mem" => Ok(Self::Mem),
            "iorun" => Ok(Self::IoRun),
            "runtime" => Ok(Self::Runtime),
            "yield" => Ok(Self::Yield),
            "barrier" => Ok(Self::Barrier),
            "fork" => Ok(Self::Fork),
            _ => Err(ParseEnumError {
                type_name: "ResourceType",
                value: s.to_owned(),
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// Resource data — replaces the C union
// ---------------------------------------------------------------------------

/// The runtime data associated with a shared resource.
///
/// In the C code this is a union (`_rtapp_resource_t.res`).  In Rust we model
/// it as an enum whose variants carry only the data relevant to that kind of
/// resource.
#[derive(Debug)]
pub enum ResourceData {
    /// A pthread-style mutex (will be backed by `parking_lot` or `std::sync`).
    Mutex,
    /// A condition variable used for wait/signal/broadcast.
    Condition,
    /// A barrier-like synchronisation point.
    Barrier {
        /// How many threads are currently waiting.
        waiting: usize,
    },
    /// A timer that fires at `next` (optionally relative).
    Timer {
        next: Option<Duration>,
        initialized: bool,
        relative: bool,
    },
    /// An I/O memory buffer.
    IoMem { size: usize },
    /// An I/O device identified by file descriptor.
    IoDev {
        // TODO(rtapp-1ec29): will hold an OwnedFd once the I/O module is ported
    },
    /// A fork-point that spawns child threads.
    Fork {
        /// Name of the thread template to fork.
        reference: String,
        /// How many forks to create.
        num_forks: u32,
    },
}

/// A named, indexed shared resource together with its runtime data.
#[derive(Debug)]
pub struct Resource {
    pub index: ResourceIndex,
    pub resource_type: ResourceType,
    pub name: String,
    pub data: ResourceData,
}

// ---------------------------------------------------------------------------
// Event data
// ---------------------------------------------------------------------------

/// An individual event (operation) within a phase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventData {
    /// Human-readable name of the event.
    pub name: String,
    /// What kind of resource operation this event performs.
    #[serde(rename = "type")]
    pub event_type: ResourceType,
    /// Index of the resource this event refers to, if applicable.
    #[serde(rename = "ref")]
    pub resource: Option<ResourceIndex>,
    /// Index of a dependent resource, if applicable.
    #[serde(rename = "dep")]
    pub dependency: Option<ResourceIndex>,
    /// Duration of the event (interpretation depends on `event_type`).
    #[serde(default)]
    pub duration: Duration,
    /// Repeat count for the event.
    #[serde(default)]
    pub count: u32,
}

// ---------------------------------------------------------------------------
// CPU-set data
// ---------------------------------------------------------------------------

/// CPU affinity information for a thread or phase.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CpuSetData {
    /// The string representation from the config (e.g. "0-3" or "0,2,4").
    #[serde(default)]
    pub cpuset_str: Option<String>,
    /// Parsed set of CPU indices.
    #[serde(skip)]
    pub cpus: Vec<usize>,
}

// ---------------------------------------------------------------------------
// NUMA-set data
// ---------------------------------------------------------------------------

/// NUMA node affinity information.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NumaSetData {
    /// The string representation from the config.
    #[serde(default)]
    pub numaset_str: Option<String>,
}

// ---------------------------------------------------------------------------
// Scheduling data
// ---------------------------------------------------------------------------

/// Scheduling parameters for a thread or phase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedData {
    /// Which scheduling policy to use.
    pub policy: SchedulingPolicy,
    /// Real-time priority (for FIFO / RR) or nice value (for OTHER).
    #[serde(default = "default_priority")]
    pub priority: i32,
    /// SCHED_DEADLINE runtime budget.
    #[serde(default)]
    pub runtime: Option<Duration>,
    /// SCHED_DEADLINE absolute deadline.
    #[serde(default)]
    pub deadline: Option<Duration>,
    /// SCHED_DEADLINE period.
    #[serde(default)]
    pub period: Option<Duration>,
    /// Minimum utilization clamp (0..=1024).
    #[serde(default)]
    pub util_min: Option<u32>,
    /// Maximum utilization clamp (0..=1024).
    #[serde(default)]
    pub util_max: Option<u32>,
}

fn default_priority() -> i32 {
    DEFAULT_THREAD_PRIORITY
}

// ---------------------------------------------------------------------------
// Task-group data
// ---------------------------------------------------------------------------

/// A cgroup-like task group assignment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskGroupData {
    pub name: String,
    #[serde(default)]
    pub offset: i32,
}

// ---------------------------------------------------------------------------
// Phase data
// ---------------------------------------------------------------------------

/// A single execution phase within a thread's loop body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseData {
    /// Number of loop iterations for this phase (0 = infinite).
    #[serde(rename = "loop", default)]
    pub loop_count: i32,
    /// The ordered sequence of events in this phase.
    #[serde(default)]
    pub events: Vec<EventData>,
    /// CPU affinity override for this phase.
    #[serde(default)]
    pub cpu_data: CpuSetData,
    /// NUMA affinity override for this phase.
    #[serde(default)]
    pub numa_data: NumaSetData,
    /// Scheduling parameter override for this phase.
    #[serde(default)]
    pub sched_data: Option<SchedData>,
    /// Task-group override for this phase.
    #[serde(default)]
    pub taskgroup_data: Option<TaskGroupData>,
}

// ---------------------------------------------------------------------------
// Thread data
// ---------------------------------------------------------------------------

/// Full configuration and runtime state for a single thread.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadData {
    /// Thread index (set at runtime).
    #[serde(skip)]
    pub index: ThreadIndex,
    /// Thread name from the config.
    pub name: String,
    /// Whether to lock pages in memory (mlockall).
    #[serde(default)]
    pub lock_pages: bool,
    /// Total duration the thread should run before stopping.
    #[serde(default)]
    pub duration: Option<Duration>,

    /// CPU affinity for the thread.
    #[serde(default)]
    pub cpu_data: CpuSetData,
    /// Default CPU affinity (restored between phases).
    #[serde(skip)]
    pub default_cpu_data: CpuSetData,

    /// NUMA affinity for the thread.
    #[serde(default)]
    pub numa_data: NumaSetData,

    /// Scheduling parameters for the thread.
    #[serde(default)]
    pub sched_data: Option<SchedData>,

    /// Task-group assignment for the thread.
    #[serde(default)]
    pub taskgroup_data: Option<TaskGroupData>,

    /// Number of loop iterations (0 = infinite).
    #[serde(rename = "loop", default)]
    pub loop_count: i32,
    /// Execution phases.
    #[serde(default)]
    pub phases: Vec<PhaseData>,

    /// Delay before the thread starts executing.
    #[serde(default)]
    pub delay: Option<Duration>,

    /// Whether this thread was created by a fork event.
    #[serde(skip)]
    pub forked: bool,
    /// Number of instances of this thread.
    #[serde(default = "default_instances")]
    pub num_instances: u32,
}

fn default_instances() -> u32 {
    1
}

// ---------------------------------------------------------------------------
// Application-wide options
// ---------------------------------------------------------------------------

/// Top-level application configuration parsed from the JSON config file and CLI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppOptions {
    /// Whether to lock all pages in memory.
    #[serde(default)]
    pub lock_pages: bool,
    /// Default scheduling policy for all threads.
    #[serde(default = "default_policy")]
    pub policy: SchedulingPolicy,
    /// Global duration limit for the entire application.
    #[serde(default)]
    pub duration: Option<Duration>,
    /// Directory for log files.
    #[serde(default)]
    pub logdir: Option<PathBuf>,
    /// Base name for per-thread log files.
    #[serde(default)]
    pub logbasename: Option<String>,
    /// Maximum log file size in bytes.
    #[serde(default)]
    pub logsize: Option<usize>,
    /// Whether to generate gnuplot output.
    #[serde(default)]
    pub gnuplot: bool,
    /// CPU to use for calibration (-1 = any).
    #[serde(default = "default_calib_cpu")]
    pub calib_cpu: i32,
    /// Calibrated nanoseconds per busy-loop iteration.
    #[serde(default)]
    pub calib_ns_per_loop: Option<u64>,
    /// Whether priority inheritance is enabled for mutexes.
    #[serde(default)]
    pub pi_enabled: bool,
    /// Whether to die on deadline miss.
    #[serde(default)]
    pub die_on_dmiss: bool,
    /// Size of the memory buffer for mem events (bytes).
    #[serde(default)]
    pub mem_buffer_size: Option<usize>,
    /// Path to the I/O device for iorun events.
    #[serde(default)]
    pub io_device: Option<PathBuf>,
    /// Whether slack is accumulated across loop iterations.
    #[serde(default)]
    pub cumulative_slack: bool,
}

fn default_policy() -> SchedulingPolicy {
    SchedulingPolicy::Other
}

fn default_calib_cpu() -> i32 {
    -1
}

// ---------------------------------------------------------------------------
// Timing / logging data
// ---------------------------------------------------------------------------

/// A single timing measurement taken at the end of each loop iteration.
#[derive(Debug, Clone, Default)]
pub struct TimingPoint {
    /// Loop iteration index.
    pub index: u32,
    /// Wall-clock performance counter (usec).
    pub perf_usec: u64,
    /// Duration of the iteration (usec).
    pub duration_usec: u64,
    /// Period between iteration starts (usec).
    pub period_usec: u64,
    /// Cumulative duration (usec).
    pub cumulative_duration_usec: u64,
    /// Cumulative period (usec).
    pub cumulative_period_usec: u64,
    /// Wake-up latency (usec).
    pub wakeup_latency_usec: u64,
    /// Slack time (may be negative on overrun).
    pub slack_nsec: i64,
    /// Absolute start time of this iteration (nsec since boot).
    pub start_time_nsec: u64,
    /// Absolute end time of this iteration (nsec since boot).
    pub end_time_nsec: u64,
    /// Relative start time from application start (nsec).
    pub rel_start_time_nsec: u64,
}

/// Accumulated per-thread log statistics.
#[derive(Debug, Clone, Default)]
pub struct LogData {
    /// Total performance counter (usec).
    pub perf_usec: u64,
    /// Total duration (usec).
    pub duration_usec: u64,
    /// Total wake-up latency (usec).
    pub wakeup_latency_usec: u64,
    /// Cumulative duration (usec).
    pub cumulative_duration_usec: u64,
    /// Cumulative period (usec).
    pub cumulative_period_usec: u64,
    /// Slack (may be negative).
    pub slack_nsec: i64,
}

// ---------------------------------------------------------------------------
// Ftrace data
// ---------------------------------------------------------------------------

/// State for the ftrace tracing subsystem.
#[derive(Debug)]
pub struct FtraceData {
    /// Path to the tracefs mount point (e.g. `/sys/kernel/tracing`).
    pub tracefs: PathBuf,
    /// File descriptor for the `trace` file.
    pub trace_fd: Option<std::os::fd::RawFd>,
    /// File descriptor for the `trace_marker` file.
    pub marker_fd: Option<std::os::fd::RawFd>,
}

// ---------------------------------------------------------------------------
// sched_attr — for the sched_setattr(2) syscall
// ---------------------------------------------------------------------------

/// The `sched_attr` structure passed to `sched_setattr(2)` / `sched_getattr(2)`.
///
/// Layout matches the kernel definition in `linux/sched/types.h`.
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct SchedAttr {
    /// Size of this structure (for versioning).
    pub size: u32,
    /// Scheduling policy (`SCHED_*` constant).
    pub sched_policy: u32,
    /// Scheduling flags (bitwise OR of `SchedFlags`).
    pub sched_flags: u64,
    /// Nice value for `SCHED_OTHER` / `SCHED_BATCH`.
    pub sched_nice: i32,
    /// Priority for `SCHED_FIFO` / `SCHED_RR`.
    pub sched_priority: u32,
    /// Runtime budget for `SCHED_DEADLINE` (nsec).
    pub sched_runtime: u64,
    /// Deadline for `SCHED_DEADLINE` (nsec).
    pub sched_deadline: u64,
    /// Period for `SCHED_DEADLINE` (nsec).
    pub sched_period: u64,
    /// Minimum utilization clamp hint.
    pub sched_util_min: u32,
    /// Maximum utilization clamp hint.
    pub sched_util_max: u32,
}

impl SchedAttr {
    /// Create a new `SchedAttr` with `size` pre-filled.
    pub fn new() -> Self {
        Self {
            size: std::mem::size_of::<Self>() as u32,
            ..Default::default()
        }
    }
}

// ---------------------------------------------------------------------------
// Bitflags: sched flags
// ---------------------------------------------------------------------------

bitflags! {
    /// Flags for the `sched_flags` field of [`SchedAttr`].
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct SchedFlags: u64 {
        const RESET_ON_FORK  = 0x01;
        const RECLAIM        = 0x02;
        const DL_OVERRUN     = 0x04;
        const KEEP_POLICY    = 0x08;
        const KEEP_PARAMS    = 0x10;
        const UTIL_CLAMP_MIN = 0x20;
        const UTIL_CLAMP_MAX = 0x40;

        const KEEP_ALL    = Self::KEEP_POLICY.bits() | Self::KEEP_PARAMS.bits();
        const UTIL_CLAMP  = Self::UTIL_CLAMP_MIN.bits() | Self::UTIL_CLAMP_MAX.bits();
        const ALL         = Self::RESET_ON_FORK.bits()
                          | Self::RECLAIM.bits()
                          | Self::DL_OVERRUN.bits()
                          | Self::KEEP_ALL.bits()
                          | Self::UTIL_CLAMP.bits();
    }
}

// ---------------------------------------------------------------------------
// Bitflags: ftrace level
// ---------------------------------------------------------------------------

bitflags! {
    /// Categories of ftrace messages that can be independently enabled.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct FtraceLevel: u32 {
        const NONE  = 0x00;
        const MAIN  = 0x01;
        const TASK  = 0x02;
        const LOOP  = 0x04;
        const EVENT = 0x08;
        const STATS = 0x10;
        const ATTRS = 0x20;
    }
}

// ---------------------------------------------------------------------------
// Error type for enum parsing
// ---------------------------------------------------------------------------

/// Error returned when a string cannot be parsed into one of our enums.
#[derive(Debug, Clone, thiserror::Error)]
#[error("invalid {type_name} value: {value:?}")]
pub struct ParseEnumError {
    pub type_name: &'static str,
    pub value: String,
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- SchedulingPolicy --------------------------------------------------

    #[test]
    fn scheduling_policy_display_roundtrip() {
        let policies = [
            SchedulingPolicy::Other,
            SchedulingPolicy::Idle,
            SchedulingPolicy::RoundRobin,
            SchedulingPolicy::Fifo,
            SchedulingPolicy::Deadline,
            SchedulingPolicy::Same,
        ];
        for p in &policies {
            let s = p.to_string();
            let parsed: SchedulingPolicy = s.parse().unwrap();
            assert_eq!(*p, parsed, "roundtrip failed for {s}");
        }
    }

    #[test]
    fn scheduling_policy_from_str_invalid() {
        let result = "SCHED_MAGIC".parse::<SchedulingPolicy>();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("SCHED_MAGIC"));
    }

    #[test]
    fn scheduling_policy_kernel_ids() {
        assert_eq!(SchedulingPolicy::Other.to_kernel_id(), Some(0));
        assert_eq!(SchedulingPolicy::Fifo.to_kernel_id(), Some(1));
        assert_eq!(SchedulingPolicy::RoundRobin.to_kernel_id(), Some(2));
        assert_eq!(SchedulingPolicy::Idle.to_kernel_id(), Some(5));
        assert_eq!(SchedulingPolicy::Deadline.to_kernel_id(), Some(6));
        assert_eq!(SchedulingPolicy::Same.to_kernel_id(), None);
    }

    #[test]
    fn scheduling_policy_serde_json() {
        let json = r#""SCHED_FIFO""#;
        let p: SchedulingPolicy = serde_json::from_str(json).unwrap();
        assert_eq!(p, SchedulingPolicy::Fifo);
        assert_eq!(serde_json::to_string(&p).unwrap(), json);
    }

    // -- ResourceType ------------------------------------------------------

    #[test]
    fn resource_type_display_roundtrip() {
        let types = [
            ResourceType::Unknown,
            ResourceType::Mutex,
            ResourceType::Wait,
            ResourceType::Signal,
            ResourceType::Broadcast,
            ResourceType::Sleep,
            ResourceType::Run,
            ResourceType::SigAndWait,
            ResourceType::Lock,
            ResourceType::Unlock,
            ResourceType::Timer,
            ResourceType::TimerUnique,
            ResourceType::Suspend,
            ResourceType::Resume,
            ResourceType::Mem,
            ResourceType::IoRun,
            ResourceType::Runtime,
            ResourceType::Yield,
            ResourceType::Barrier,
            ResourceType::Fork,
        ];
        for rt in &types {
            let s = rt.to_string();
            let parsed: ResourceType = s.parse().unwrap();
            assert_eq!(*rt, parsed, "roundtrip failed for {s}");
        }
    }

    #[test]
    fn resource_type_from_str_invalid() {
        let result = "magic_resource".parse::<ResourceType>();
        assert!(result.is_err());
    }

    #[test]
    fn resource_type_serde_json() {
        let json = r#""timer""#;
        let rt: ResourceType = serde_json::from_str(json).unwrap();
        assert_eq!(rt, ResourceType::Timer);
        assert_eq!(serde_json::to_string(&rt).unwrap(), json);
    }

    // -- SchedFlags --------------------------------------------------------

    #[test]
    fn sched_flags_combinations() {
        let flags = SchedFlags::KEEP_POLICY | SchedFlags::KEEP_PARAMS;
        assert_eq!(flags, SchedFlags::KEEP_ALL);

        let util = SchedFlags::UTIL_CLAMP_MIN | SchedFlags::UTIL_CLAMP_MAX;
        assert_eq!(util, SchedFlags::UTIL_CLAMP);
    }

    #[test]
    fn sched_flags_values_match_c() {
        assert_eq!(SchedFlags::RESET_ON_FORK.bits(), 0x01);
        assert_eq!(SchedFlags::RECLAIM.bits(), 0x02);
        assert_eq!(SchedFlags::DL_OVERRUN.bits(), 0x04);
        assert_eq!(SchedFlags::KEEP_POLICY.bits(), 0x08);
        assert_eq!(SchedFlags::KEEP_PARAMS.bits(), 0x10);
        assert_eq!(SchedFlags::UTIL_CLAMP_MIN.bits(), 0x20);
        assert_eq!(SchedFlags::UTIL_CLAMP_MAX.bits(), 0x40);
    }

    // -- FtraceLevel -------------------------------------------------------

    #[test]
    fn ftrace_level_values_match_c() {
        assert_eq!(FtraceLevel::NONE.bits(), 0x00);
        assert_eq!(FtraceLevel::MAIN.bits(), 0x01);
        assert_eq!(FtraceLevel::TASK.bits(), 0x02);
        assert_eq!(FtraceLevel::LOOP.bits(), 0x04);
        assert_eq!(FtraceLevel::EVENT.bits(), 0x08);
        assert_eq!(FtraceLevel::STATS.bits(), 0x10);
        assert_eq!(FtraceLevel::ATTRS.bits(), 0x20);
    }

    #[test]
    fn ftrace_level_combine() {
        let level = FtraceLevel::MAIN | FtraceLevel::TASK | FtraceLevel::LOOP;
        assert!(level.contains(FtraceLevel::MAIN));
        assert!(level.contains(FtraceLevel::TASK));
        assert!(level.contains(FtraceLevel::LOOP));
        assert!(!level.contains(FtraceLevel::EVENT));
    }

    // -- SchedAttr ---------------------------------------------------------

    #[test]
    fn sched_attr_new_has_correct_size() {
        let attr = SchedAttr::new();
        assert_eq!(attr.size as usize, std::mem::size_of::<SchedAttr>());
    }

    // -- ExitCode ----------------------------------------------------------

    #[test]
    fn exit_code_values() {
        assert_eq!(i32::from(ExitCode::Success), 0);
        assert_eq!(i32::from(ExitCode::Failure), 1);
        assert_eq!(i32::from(ExitCode::InvalidConfig), 2);
        assert_eq!(i32::from(ExitCode::InvalidCommandLine), 3);
    }

    // -- Newtype indices ---------------------------------------------------

    #[test]
    fn index_display() {
        assert_eq!(ThreadIndex(3).to_string(), "3");
        assert_eq!(ResourceIndex(42).to_string(), "42");
        assert_eq!(PhaseIndex(0).to_string(), "0");
        assert_eq!(EventIndex(99).to_string(), "99");
    }

    #[test]
    fn index_serde_roundtrip() {
        let idx = ThreadIndex(7);
        let json = serde_json::to_string(&idx).unwrap();
        assert_eq!(json, "7");
        let parsed: ThreadIndex = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, idx);
    }

    // -- Defaults ----------------------------------------------------------

    #[test]
    fn timing_point_default() {
        let tp = TimingPoint::default();
        assert_eq!(tp.index, 0);
        assert_eq!(tp.slack_nsec, 0);
    }

    #[test]
    fn log_data_default() {
        let ld = LogData::default();
        assert_eq!(ld.perf_usec, 0);
        assert_eq!(ld.slack_nsec, 0);
    }

    // -- ParseEnumError ----------------------------------------------------

    #[test]
    fn parse_enum_error_display() {
        let err = ParseEnumError {
            type_name: "TestEnum",
            value: "bad".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("TestEnum"));
        assert!(msg.contains("bad"));
    }
}
