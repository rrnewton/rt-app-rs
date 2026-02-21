//! Random JSON workload generator for rt-app.
//!
//! Produces valid rt-app JSON configurations with safety limits to prevent
//! runaway workloads.

use rand::prelude::*;
use serde_json::{json, Map, Value};

// ---------------------------------------------------------------------------
// Safety limits - hardcoded defaults
// ---------------------------------------------------------------------------

/// Default safety limits for generated workloads.
#[derive(Debug, Clone)]
pub struct SafetyLimits {
    /// Maximum number of threads per workload.
    pub max_threads: u32,
    /// Maximum number of instances per thread.
    pub max_instances: u32,
    /// Maximum number of phases per thread.
    pub max_phases: u32,
    /// Maximum number of events per phase.
    pub max_events: u32,
    /// Maximum global duration in seconds.
    pub max_duration_secs: u32,
    /// Maximum run event duration in microseconds.
    pub max_run_us: u32,
    /// Maximum sleep event duration in microseconds.
    pub max_sleep_us: u32,
    /// Maximum timer period in microseconds.
    pub max_timer_period_us: u32,
    /// Maximum loop count per phase/thread.
    pub max_loop_count: u32,
    /// Maximum memory buffer size in bytes.
    pub max_mem_bytes: u32,
}

impl Default for SafetyLimits {
    fn default() -> Self {
        Self {
            max_threads: 4,
            max_instances: 2,
            max_phases: 3,
            max_events: 6,
            max_duration_secs: 2,
            max_run_us: 50_000,
            max_sleep_us: 50_000,
            max_timer_period_us: 100_000,
            max_loop_count: 10,
            max_mem_bytes: 1024 * 1024, // 1 MiB
        }
    }
}

// ---------------------------------------------------------------------------
// Generator configuration
// ---------------------------------------------------------------------------

/// Configuration for the workload generator.
#[derive(Debug, Clone, Default)]
pub struct GeneratorConfig {
    /// Safety limits for generated workloads.
    pub limits: SafetyLimits,
    /// Enable rt-app-rs-only features (future use).
    #[allow(dead_code)] // Will be used for rt-app-rs-only features
    pub extended: bool,
}

// ---------------------------------------------------------------------------
// Strong types for clarity
// ---------------------------------------------------------------------------

/// Duration in microseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Microseconds(u32);

impl Microseconds {
    fn as_u32(self) -> u32 {
        self.0
    }
}

/// A resource name (mutex, barrier, timer, etc.).
#[derive(Debug, Clone)]
struct ResourceName(String);

impl ResourceName {
    fn as_str(&self) -> &str {
        &self.0
    }
}

// ---------------------------------------------------------------------------
// Generator state
// ---------------------------------------------------------------------------

/// Tracks resources and state during workload generation.
struct GeneratorState {
    /// Names of mutexes available for lock/unlock events.
    mutexes: Vec<ResourceName>,
    /// Names of barriers available for barrier events.
    barriers: Vec<ResourceName>,
    /// Names of timers available for timer events.
    timers: Vec<ResourceName>,
    /// Thread names for barrier coordination.
    thread_names: Vec<String>,
}

impl GeneratorState {
    fn new() -> Self {
        Self {
            mutexes: Vec::new(),
            barriers: Vec::new(),
            timers: Vec::new(),
            thread_names: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Workload generator
// ---------------------------------------------------------------------------

/// Generates random valid rt-app JSON workloads.
pub struct WorkloadGenerator {
    config: GeneratorConfig,
    rng: StdRng,
}

impl WorkloadGenerator {
    /// Create a new generator with the given config and seed.
    pub fn new(config: GeneratorConfig, seed: u64) -> Self {
        Self {
            config,
            rng: StdRng::seed_from_u64(seed),
        }
    }

    /// Generate a random valid workload and return it as a JSON value.
    pub fn generate(&mut self) -> Value {
        let mut state = GeneratorState::new();

        // Pre-generate thread names
        let num_threads = self.random_range(1, self.config.limits.max_threads + 1);
        for i in 0..num_threads {
            state.thread_names.push(format!("thread{}", i));
        }

        // Generate resources section
        let resources = self.generate_resources(&mut state);

        // Generate tasks section
        let tasks = self.generate_tasks(&mut state);

        // Generate global section
        let global = self.generate_global();

        let mut workload = Map::new();
        if !resources.is_empty() {
            workload.insert("resources".to_string(), Value::Object(resources));
        }
        workload.insert("tasks".to_string(), Value::Object(tasks));
        workload.insert("global".to_string(), Value::Object(global));

        Value::Object(workload)
    }

    fn generate_global(&mut self) -> Map<String, Value> {
        let mut global = Map::new();

        // Duration: random between 0.5 and max_duration
        let duration_secs = self.random_range(1, self.config.limits.max_duration_secs + 1);
        global.insert("duration".to_string(), json!(duration_secs));

        // Calibration: "CPU0" or a small integer
        if self.coin_flip() {
            global.insert("calibration".to_string(), json!("CPU0"));
        } else {
            let cpu = self.random_range(0, 4);
            global.insert("calibration".to_string(), json!(format!("CPU{}", cpu)));
        }

        // Default policy: mostly SCHED_OTHER
        let policy = self.random_policy();
        global.insert("default_policy".to_string(), json!(policy));

        // PI enabled: random boolean
        global.insert("pi_enabled".to_string(), json!(self.coin_flip()));

        // Lock pages: random boolean
        global.insert("lock_pages".to_string(), json!(self.coin_flip()));

        // Gnuplot: always false for automated testing
        global.insert("gnuplot".to_string(), json!(false));

        global
    }

    fn generate_resources(&mut self, state: &mut GeneratorState) -> Map<String, Value> {
        let mut resources = Map::new();

        // Generate 0-3 mutexes
        let num_mutexes = self.random_range(0, 4);
        for i in 0..num_mutexes {
            let name = format!("mutex{}", i);
            resources.insert(name.clone(), json!({"type": "mutex"}));
            state.mutexes.push(ResourceName(name));
        }

        // Generate 0-2 barriers (need multiple threads to be useful)
        if state.thread_names.len() > 1 {
            let num_barriers = self.random_range(0, 3);
            for i in 0..num_barriers {
                let name = format!("barrier{}", i);
                resources.insert(name.clone(), json!({"type": "barrier"}));
                state.barriers.push(ResourceName(name));
            }
        }

        // Generate 0-2 timers
        let num_timers = self.random_range(0, 3);
        for i in 0..num_timers {
            let name = format!("timer{}", i);
            resources.insert(name.clone(), json!({"type": "timer"}));
            state.timers.push(ResourceName(name));
        }

        resources
    }

    fn generate_tasks(&mut self, state: &mut GeneratorState) -> Map<String, Value> {
        let mut tasks = Map::new();

        for thread_name in state.thread_names.clone() {
            let thread = self.generate_thread(state);
            tasks.insert(thread_name, thread);
        }

        tasks
    }

    fn generate_thread(&mut self, state: &mut GeneratorState) -> Value {
        let mut thread = Map::new();

        // Instance count
        let instances = self.random_range(1, self.config.limits.max_instances + 1);
        if instances > 1 {
            thread.insert("instance".to_string(), json!(instances));
        }

        // Policy: occasionally use RT policies with low priority
        let policy = self.random_policy();
        thread.insert("policy".to_string(), json!(policy));

        // Priority for RT policies
        if policy == "SCHED_FIFO" || policy == "SCHED_RR" {
            let priority = self.random_range(1, 10); // Low priority for safety
            thread.insert("priority".to_string(), json!(priority));
        }

        // CPU affinity: occasionally set
        if self.one_in_n(3) {
            let cpu = self.random_range(0, 4);
            thread.insert("cpus".to_string(), json!([cpu]));
        }

        // Loop count
        let loop_count = if self.one_in_n(4) {
            -1 // Infinite (will be bounded by global duration)
        } else {
            self.random_range(1, self.config.limits.max_loop_count + 1) as i32
        };
        thread.insert("loop".to_string(), json!(loop_count));

        // Decide whether to use phases or simple events
        if self.coin_flip() {
            // Use phases
            let phases = self.generate_phases(state);
            thread.insert("phases".to_string(), Value::Object(phases));
        } else {
            // Simple: just run + sleep/timer
            self.add_simple_events(&mut thread, state);
        }

        Value::Object(thread)
    }

    fn generate_phases(&mut self, state: &mut GeneratorState) -> Map<String, Value> {
        let mut phases = Map::new();
        let num_phases = self.random_range(1, self.config.limits.max_phases + 1);

        for i in 0..num_phases {
            let phase_name = format!("phase{}", i);
            let phase = self.generate_phase(state);
            phases.insert(phase_name, phase);
        }

        phases
    }

    fn generate_phase(&mut self, state: &mut GeneratorState) -> Value {
        let mut phase = Map::new();

        // Phase loop count
        let loop_count = self.random_range(1, self.config.limits.max_loop_count + 1);
        phase.insert("loop".to_string(), json!(loop_count));

        // Generate events for this phase
        self.add_phase_events(&mut phase, state);

        Value::Object(phase)
    }

    /// Add simple run + sleep/timer events to a thread or phase.
    fn add_simple_events(&mut self, target: &mut Map<String, Value>, state: &GeneratorState) {
        // Run event
        let run_us = self.random_run_duration();
        target.insert("run".to_string(), json!(run_us.as_u32()));

        // Sleep or timer
        if self.coin_flip() || state.timers.is_empty() {
            let sleep_us = self.random_sleep_duration();
            target.insert("sleep".to_string(), json!(sleep_us.as_u32()));
        } else {
            let timer = self.random_element(&state.timers);
            // Timer period must be > run time
            let period = self.random_timer_period(run_us);
            target.insert(
                "timer".to_string(),
                json!({
                    "ref": timer.as_str(),
                    "period": period.as_u32()
                }),
            );
        }
    }

    /// Add events to a phase, tracking state for proper lock/unlock pairing.
    fn add_phase_events(&mut self, phase: &mut Map<String, Value>, state: &GeneratorState) {
        let mut event_counter = 0;
        let max_events = self.random_range(1, self.config.limits.max_events + 1);
        let mut total_runtime = Microseconds(0);
        let mut held_locks: Vec<&ResourceName> = Vec::new();

        while event_counter < max_events {
            // Choose event type based on what makes sense
            let event_type = self.choose_event_type(state, &held_locks);

            match event_type.as_str() {
                "run" => {
                    let run_us = self.random_run_duration();
                    total_runtime = Microseconds(total_runtime.0 + run_us.0);
                    phase.insert(format!("run{}", event_counter), json!(run_us.as_u32()));
                }
                "runtime" => {
                    let run_us = self.random_run_duration();
                    total_runtime = Microseconds(total_runtime.0 + run_us.0);
                    phase.insert(format!("runtime{}", event_counter), json!(run_us.as_u32()));
                }
                "sleep" => {
                    let sleep_us = self.random_sleep_duration();
                    phase.insert(format!("sleep{}", event_counter), json!(sleep_us.as_u32()));
                }
                "lock" => {
                    if let Some(mutex) = self.maybe_random_element(&state.mutexes) {
                        phase.insert(format!("lock{}", event_counter), json!(mutex.as_str()));
                        held_locks.push(mutex);
                    }
                }
                "unlock" => {
                    if let Some(mutex) = held_locks.pop() {
                        phase.insert(format!("unlock{}", event_counter), json!(mutex.as_str()));
                    }
                }
                "barrier" => {
                    if let Some(barrier) = self.maybe_random_element(&state.barriers) {
                        phase.insert(format!("barrier{}", event_counter), json!(barrier.as_str()));
                    }
                }
                "mem" => {
                    let size = self.random_mem_size();
                    phase.insert(format!("mem{}", event_counter), json!(size));
                }
                "yield" => {
                    phase.insert(format!("yield{}", event_counter), json!(null));
                }
                _ => {}
            }

            event_counter += 1;
        }

        // Close any remaining locks (proper nesting)
        for mutex in held_locks.into_iter().rev() {
            phase.insert(format!("unlock{}", event_counter), json!(mutex.as_str()));
            event_counter += 1;
        }

        // Add timer at the end if we have timers and some runtime
        if total_runtime.0 > 0 && !state.timers.is_empty() && self.coin_flip() {
            let timer = self.random_element(&state.timers);
            let period = self.random_timer_period(total_runtime);
            phase.insert(
                format!("timer{}", event_counter),
                json!({
                    "ref": timer.as_str(),
                    "period": period.as_u32()
                }),
            );
        }
    }

    fn choose_event_type(
        &mut self,
        state: &GeneratorState,
        held_locks: &[&ResourceName],
    ) -> String {
        // Weighted event selection
        let mut choices: Vec<(&str, u32)> = vec![("run", 30), ("runtime", 20), ("sleep", 20)];

        // Add lock if we have mutexes and haven't held too many
        if !state.mutexes.is_empty() && held_locks.len() < 2 {
            choices.push(("lock", 5));
        }

        // Add unlock if we're holding locks
        if !held_locks.is_empty() {
            choices.push(("unlock", 10));
        }

        // Add barrier if available
        if !state.barriers.is_empty() {
            choices.push(("barrier", 5));
        }

        // Occasional mem and yield
        choices.push(("mem", 3));
        choices.push(("yield", 2));

        self.weighted_choice(&choices)
    }

    // -----------------------------------------------------------------------
    // Random helpers
    // -----------------------------------------------------------------------

    fn random_range(&mut self, min: u32, max: u32) -> u32 {
        if min >= max {
            min
        } else {
            self.rng.gen_range(min..max)
        }
    }

    fn coin_flip(&mut self) -> bool {
        self.rng.gen_bool(0.5)
    }

    fn one_in_n(&mut self, n: u32) -> bool {
        self.rng.gen_ratio(1, n)
    }

    fn random_policy(&mut self) -> &'static str {
        // Mostly SCHED_OTHER for safety
        let roll = self.random_range(0, 100);
        if roll < 80 {
            "SCHED_OTHER"
        } else if roll < 90 {
            "SCHED_FIFO"
        } else {
            "SCHED_RR"
        }
    }

    fn random_run_duration(&mut self) -> Microseconds {
        let us = self.random_range(100, self.config.limits.max_run_us + 1);
        Microseconds(us)
    }

    fn random_sleep_duration(&mut self) -> Microseconds {
        let us = self.random_range(100, self.config.limits.max_sleep_us + 1);
        Microseconds(us)
    }

    fn random_timer_period(&mut self, min_runtime: Microseconds) -> Microseconds {
        // Period must be greater than runtime
        let min_period = min_runtime.0.saturating_add(1000);
        let max_period = self
            .config
            .limits
            .max_timer_period_us
            .max(min_period + 1000);
        let us = self.random_range(min_period, max_period + 1);
        Microseconds(us)
    }

    fn random_mem_size(&mut self) -> u32 {
        // 64 bytes to 4096 bytes
        let max = self.config.limits.max_mem_bytes.min(4096);
        self.random_range(64, max + 1)
    }

    fn random_element<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        let idx = self.rng.gen_range(0..items.len());
        &items[idx]
    }

    fn maybe_random_element<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() {
            None
        } else {
            Some(self.random_element(items))
        }
    }

    fn weighted_choice(&mut self, choices: &[(&str, u32)]) -> String {
        let total: u32 = choices.iter().map(|(_, w)| w).sum();
        let mut roll = self.random_range(0, total);

        for (name, weight) in choices {
            if roll < *weight {
                return (*name).to_string();
            }
            roll -= weight;
        }

        // Fallback
        choices[0].0.to_string()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_produces_valid_json() {
        let config = GeneratorConfig::default();
        let mut gen = WorkloadGenerator::new(config, 12345);

        let workload = gen.generate();

        // Should have tasks and global sections
        assert!(workload.get("tasks").is_some());
        assert!(workload.get("global").is_some());

        // Global should have duration
        let global = workload.get("global").unwrap();
        assert!(global.get("duration").is_some());
    }

    #[test]
    fn generate_respects_thread_limit() {
        let mut config = GeneratorConfig::default();
        config.limits.max_threads = 2;

        let mut gen = WorkloadGenerator::new(config, 42);

        for _ in 0..10 {
            let workload = gen.generate();
            let tasks = workload.get("tasks").unwrap().as_object().unwrap();
            assert!(tasks.len() <= 2);
        }
    }

    #[test]
    fn generate_is_deterministic_with_same_seed() {
        let config = GeneratorConfig::default();

        let mut gen1 = WorkloadGenerator::new(config.clone(), 99999);
        let mut gen2 = WorkloadGenerator::new(config, 99999);

        let w1 = gen1.generate();
        let w2 = gen2.generate();

        assert_eq!(w1, w2);
    }

    #[test]
    fn generate_produces_different_output_with_different_seeds() {
        let config = GeneratorConfig::default();

        let mut gen1 = WorkloadGenerator::new(config.clone(), 111);
        let mut gen2 = WorkloadGenerator::new(config, 222);

        let w1 = gen1.generate();
        let w2 = gen2.generate();

        // Very unlikely to be equal with different seeds
        assert_ne!(w1, w2);
    }
}
