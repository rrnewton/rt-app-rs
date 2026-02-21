//! Type conversions from parsed config to runtime types.
//!
//! This module provides conversion functions and `From` implementations to
//! transform the configuration types (parsed from JSON) into the runtime
//! types used by the engine.

use std::sync::{Condvar, Mutex};
use std::time::Duration;

use crate::config::global::{Calibration, GlobalConfig, LogSize};
use crate::config::resources::{ResourceConfig, ResourceTable};
use crate::config::tasks::{EventConfig, PhaseConfig, SchedConfig, TaskConfig};
use crate::engine::events::{BarrierState, ResourceHandle, TimerState};
use crate::types::{
    AppOptions, CpuSetData, EventData, NumaSetData, PhaseData, ResourceIndex, ResourceType,
    SchedData, TaskGroupData, ThreadData, ThreadIndex,
};

// ---------------------------------------------------------------------------
// GlobalConfig -> AppOptions
// ---------------------------------------------------------------------------

/// Convert a [`GlobalConfig`] into [`AppOptions`].
impl From<&GlobalConfig> for AppOptions {
    fn from(g: &GlobalConfig) -> Self {
        let duration = if g.duration_secs < 0 {
            None
        } else {
            Some(Duration::from_secs(g.duration_secs as u64))
        };

        let (calib_cpu, calib_ns_per_loop) = match &g.calibration {
            Calibration::Cpu(cpu) => (*cpu as i32, None),
            Calibration::NsPerLoop(ns) => (-1, Some(*ns)),
            Calibration::Precise => (-1, None),
        };

        let logsize = match &g.log_size {
            LogSize::File => None,
            LogSize::Disabled => Some(0),
            LogSize::BufferBytes(bytes) => Some(*bytes),
        };

        AppOptions {
            lock_pages: g.lock_pages,
            policy: g.default_policy,
            duration,
            logdir: Some(g.logdir.clone()),
            logbasename: Some(g.log_basename.clone()),
            logsize,
            gnuplot: g.gnuplot,
            calib_cpu,
            calib_ns_per_loop,
            pi_enabled: g.pi_enabled,
            die_on_dmiss: false,
            mem_buffer_size: Some(g.mem_buffer_size),
            io_device: Some(g.io_device.clone()),
            cumulative_slack: g.cumulative_slack,
            precise_mode: matches!(g.calibration, Calibration::Precise),
        }
    }
}

// ---------------------------------------------------------------------------
// SchedConfig -> SchedData
// ---------------------------------------------------------------------------

/// Convert a [`SchedConfig`] into [`SchedData`].
impl From<&SchedConfig> for SchedData {
    fn from(s: &SchedConfig) -> Self {
        let runtime = if s.dl_runtime_usec > 0 {
            Some(Duration::from_micros(s.dl_runtime_usec as u64))
        } else {
            None
        };
        let period = if s.dl_period_usec > 0 {
            Some(Duration::from_micros(s.dl_period_usec as u64))
        } else {
            None
        };
        let deadline = if s.dl_deadline_usec > 0 {
            Some(Duration::from_micros(s.dl_deadline_usec as u64))
        } else {
            None
        };

        SchedData {
            policy: s.policy,
            priority: s.priority,
            runtime,
            deadline,
            period,
            util_min: s.util_min,
            util_max: s.util_max,
        }
    }
}

// ---------------------------------------------------------------------------
// EventConfig -> EventData
// ---------------------------------------------------------------------------

/// Convert an [`EventConfig`] into [`EventData`].
impl From<&EventConfig> for EventData {
    fn from(e: &EventConfig) -> Self {
        EventData {
            name: e.name.clone(),
            event_type: e.event_type,
            resource: e.resource,
            dependency: e.dependency,
            duration: Duration::from_micros(e.duration_usec.max(0) as u64),
            count: e.count,
        }
    }
}

// ---------------------------------------------------------------------------
// PhaseConfig -> PhaseData
// ---------------------------------------------------------------------------

/// Convert a [`PhaseConfig`] into [`PhaseData`].
impl From<&PhaseConfig> for PhaseData {
    fn from(p: &PhaseConfig) -> Self {
        let cpu_data = CpuSetData {
            cpuset_str: if p.cpus.is_empty() {
                None
            } else {
                Some(format_cpu_list(&p.cpus))
            },
            cpus: p.cpus.iter().map(|&c| c as usize).collect(),
        };

        let numa_data = NumaSetData {
            numaset_str: if p.nodes_membind.is_empty() {
                None
            } else {
                Some(format_cpu_list(&p.nodes_membind))
            },
        };

        let sched_data = p.sched.as_ref().map(SchedData::from);

        let taskgroup_data = p.taskgroup.as_ref().map(|tg| TaskGroupData {
            name: tg.clone(),
            offset: 0,
        });

        let events = p.events.iter().map(EventData::from).collect();

        PhaseData {
            loop_count: p.loop_count,
            events,
            cpu_data,
            numa_data,
            sched_data,
            taskgroup_data,
        }
    }
}

// ---------------------------------------------------------------------------
// TaskConfig -> ThreadData
// ---------------------------------------------------------------------------

/// Convert a [`TaskConfig`] into [`ThreadData`].
pub fn task_config_to_thread_data(
    t: &TaskConfig,
    global: &GlobalConfig,
    index: ThreadIndex,
) -> ThreadData {
    let cpu_data = CpuSetData {
        cpuset_str: if t.cpus.is_empty() {
            None
        } else {
            Some(format_cpu_list(&t.cpus))
        },
        cpus: t.cpus.iter().map(|&c| c as usize).collect(),
    };

    let numa_data = NumaSetData {
        numaset_str: if t.nodes_membind.is_empty() {
            None
        } else {
            Some(format_cpu_list(&t.nodes_membind))
        },
    };

    let sched_data = t.sched.as_ref().map(SchedData::from);

    let taskgroup_data = t.taskgroup.as_ref().map(|tg| TaskGroupData {
        name: tg.clone(),
        offset: 0,
    });

    let delay = if t.delay_usec > 0 {
        Some(Duration::from_micros(t.delay_usec as u64))
    } else {
        None
    };

    let duration = if global.duration_secs >= 0 {
        Some(Duration::from_secs(global.duration_secs as u64))
    } else {
        None
    };

    let phases = t.phases.iter().map(PhaseData::from).collect();

    ThreadData {
        index,
        name: t.name.clone(),
        lock_pages: global.lock_pages,
        duration,
        cpu_data: cpu_data.clone(),
        default_cpu_data: cpu_data,
        numa_data,
        sched_data,
        taskgroup_data,
        loop_count: t.loop_count,
        phases,
        delay,
        forked: false,
        num_instances: t.num_instances,
    }
}

// ---------------------------------------------------------------------------
// ResourceTable -> Vec<ResourceHandle>
// ---------------------------------------------------------------------------

/// Tracks barrier usage counts across all tasks for proper initialization.
pub struct BarrierCounter {
    counts: Vec<u32>,
}

impl BarrierCounter {
    /// Count how many thread instances use each barrier resource.
    pub fn from_tasks(tasks: &[TaskConfig], resources: &ResourceTable) -> Self {
        let mut counts = vec![0u32; resources.len()];

        for task in tasks {
            // Skip instance=0 tasks (they are forked, not spawned at startup)
            if task.num_instances == 0 {
                continue;
            }

            for phase in &task.phases {
                for event in &phase.events {
                    if event.event_type == ResourceType::Barrier {
                        if let Some(idx) = event.resource {
                            // Each instance of this task will hit the barrier
                            counts[idx.0] += task.num_instances;
                        }
                    }
                }
            }
        }

        Self { counts }
    }

    /// Get the barrier count for a resource index.
    pub fn get(&self, idx: ResourceIndex) -> u32 {
        self.counts.get(idx.0).copied().unwrap_or(1).max(1)
    }
}

/// Convert a [`ResourceTable`] into runtime [`ResourceHandle`]s.
pub fn build_resource_handles(
    resources: &ResourceTable,
    barrier_counter: &BarrierCounter,
    mem_buffer_size: usize,
) -> Vec<ResourceHandle> {
    resources
        .iter()
        .map(|rc| resource_config_to_handle(rc, barrier_counter, mem_buffer_size))
        .collect()
}

fn resource_config_to_handle(
    rc: &ResourceConfig,
    barrier_counter: &BarrierCounter,
    mem_buffer_size: usize,
) -> ResourceHandle {
    match rc.resource_type {
        ResourceType::Mutex | ResourceType::Lock | ResourceType::Unlock => {
            ResourceHandle::Mutex(Mutex::new(()))
        }
        ResourceType::Wait
        | ResourceType::Signal
        | ResourceType::Broadcast
        | ResourceType::SigAndWait
        | ResourceType::Suspend
        | ResourceType::Resume => ResourceHandle::Condvar {
            cond: Condvar::new(),
            mutex: Mutex::new(()),
        },
        ResourceType::Timer => ResourceHandle::Timer(Mutex::new(TimerState::default())),
        ResourceType::TimerUnique => ResourceHandle::Timer(Mutex::new(TimerState {
            initialized: false,
            relative: true,
            next_ns: 0,
        })),
        ResourceType::Barrier => {
            let total = barrier_counter.get(rc.index) as usize;
            ResourceHandle::Barrier {
                mutex: Mutex::new(BarrierState {
                    waiting: total,
                    total,
                }),
                cond: Condvar::new(),
            }
        }
        ResourceType::Mem | ResourceType::IoRun => ResourceHandle::IoMem {
            buf: Mutex::new(vec![0u8; mem_buffer_size]),
        },
        ResourceType::Fork => ResourceHandle::Fork {
            reference: rc.name.clone(),
            fork_count: Mutex::new(0),
        },
        _ => ResourceHandle::Mutex(Mutex::new(())),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Format a list of CPU/node indices as a comma-separated string.
fn format_cpu_list(cpus: &[u32]) -> String {
    cpus.iter()
        .map(|c| c.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// Determine the log buffer capacity from the global config.
pub fn log_capacity_from_config(global: &GlobalConfig) -> usize {
    match &global.log_size {
        LogSize::Disabled => 0,
        LogSize::File => 10_000, // Default capacity when writing to file
        LogSize::BufferBytes(bytes) => {
            // Each TimingPoint is roughly 88 bytes
            let per_point = 88;
            (*bytes / per_point).max(1)
        }
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::global::FtraceConfig;
    use crate::types::{FtraceLevel, SchedulingPolicy};
    use std::path::PathBuf;

    fn make_global_config() -> GlobalConfig {
        GlobalConfig {
            duration_secs: 5,
            default_policy: SchedulingPolicy::Fifo,
            calibration: Calibration::Cpu(2),
            log_size: LogSize::BufferBytes(2 << 20),
            logdir: PathBuf::from("/tmp"),
            log_basename: "test".into(),
            ftrace: FtraceConfig(FtraceLevel::NONE),
            lock_pages: true,
            pi_enabled: true,
            io_device: PathBuf::from("/dev/null"),
            mem_buffer_size: 1024 * 1024,
            cumulative_slack: true,
            gnuplot: true,
        }
    }

    #[test]
    fn global_config_to_app_options() {
        let g = make_global_config();
        let opts = AppOptions::from(&g);

        assert_eq!(opts.duration, Some(Duration::from_secs(5)));
        assert_eq!(opts.policy, SchedulingPolicy::Fifo);
        assert_eq!(opts.calib_cpu, 2);
        assert!(opts.calib_ns_per_loop.is_none());
        assert!(opts.lock_pages);
        assert!(opts.pi_enabled);
        assert!(opts.gnuplot);
        assert!(opts.cumulative_slack);
    }

    #[test]
    fn global_config_ns_per_loop_calibration() {
        let mut g = make_global_config();
        g.calibration = Calibration::NsPerLoop(500);
        let opts = AppOptions::from(&g);

        assert_eq!(opts.calib_cpu, -1);
        assert_eq!(opts.calib_ns_per_loop, Some(500));
    }

    #[test]
    fn global_config_infinite_duration() {
        let mut g = make_global_config();
        g.duration_secs = -1;
        let opts = AppOptions::from(&g);

        assert!(opts.duration.is_none());
    }

    #[test]
    fn sched_config_to_sched_data() {
        let sc = SchedConfig {
            policy: SchedulingPolicy::Deadline,
            priority: 0,
            dl_runtime_usec: 1000,
            dl_period_usec: 10000,
            dl_deadline_usec: 5000,
            util_min: Some(256),
            util_max: Some(768),
        };
        let sd = SchedData::from(&sc);

        assert_eq!(sd.policy, SchedulingPolicy::Deadline);
        assert_eq!(sd.runtime, Some(Duration::from_micros(1000)));
        assert_eq!(sd.period, Some(Duration::from_micros(10000)));
        assert_eq!(sd.deadline, Some(Duration::from_micros(5000)));
        assert_eq!(sd.util_min, Some(256));
        assert_eq!(sd.util_max, Some(768));
    }

    #[test]
    fn event_config_to_event_data() {
        let ec = EventConfig {
            key: "run1".into(),
            name: "run1".into(),
            event_type: ResourceType::Run,
            resource: None,
            dependency: None,
            duration_usec: 5000,
            count: 0,
        };
        let ed = EventData::from(&ec);

        assert_eq!(ed.name, "run1");
        assert_eq!(ed.event_type, ResourceType::Run);
        assert_eq!(ed.duration, Duration::from_micros(5000));
    }

    #[test]
    fn phase_config_to_phase_data() {
        let pc = PhaseConfig {
            loop_count: 5,
            events: vec![EventConfig {
                key: "sleep".into(),
                name: "sleep".into(),
                event_type: ResourceType::Sleep,
                resource: None,
                dependency: None,
                duration_usec: 1000,
                count: 0,
            }],
            cpus: vec![0, 1],
            nodes_membind: vec![],
            sched: None,
            taskgroup: Some("/tg1".into()),
        };
        let pd = PhaseData::from(&pc);

        assert_eq!(pd.loop_count, 5);
        assert_eq!(pd.events.len(), 1);
        assert_eq!(pd.cpu_data.cpus, vec![0, 1]);
        assert_eq!(
            pd.taskgroup_data.as_ref().map(|t| t.name.as_str()),
            Some("/tg1")
        );
    }

    #[test]
    fn format_cpu_list_works() {
        assert_eq!(format_cpu_list(&[0, 2, 4]), "0,2,4");
        assert_eq!(format_cpu_list(&[]), "");
        assert_eq!(format_cpu_list(&[7]), "7");
    }

    #[test]
    fn log_capacity_disabled() {
        let mut g = make_global_config();
        g.log_size = LogSize::Disabled;
        assert_eq!(log_capacity_from_config(&g), 0);
    }

    #[test]
    fn log_capacity_file() {
        let mut g = make_global_config();
        g.log_size = LogSize::File;
        assert_eq!(log_capacity_from_config(&g), 10_000);
    }

    #[test]
    fn log_capacity_buffer() {
        let mut g = make_global_config();
        g.log_size = LogSize::BufferBytes(8800); // 100 * 88 bytes
        assert_eq!(log_capacity_from_config(&g), 100);
    }
}
