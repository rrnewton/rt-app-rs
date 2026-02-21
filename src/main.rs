//! rt-app-rs: A pure Rust port of the rt-app real-time workload simulator.
//!
//! This is the main entry point that orchestrates the full pipeline:
//! 1. Parse CLI arguments
//! 2. Read and parse JSON configuration
//! 3. Convert config types to runtime types
//! 4. Run CPU calibration (unless ns_per_loop is given directly)
//! 5. Build the engine state
//! 6. Install signal handlers for graceful shutdown
//! 7. Spawn worker threads
//! 8. Wait for duration timeout or threads to finish
//! 9. Graceful shutdown

use std::path::Path;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use std::thread;
use std::time::Duration;

use clap::Parser;
use log::{debug, error, info, warn};
use nix::time::{clock_gettime, ClockId};

use rt_app_rs::args::{Cli, ConfigSource};
use rt_app_rs::config::{parse_config, parse_config_stdin, ConfigError, RtAppConfig};
use rt_app_rs::conversions::{
    build_resource_handles, log_capacity_from_config, task_config_to_thread_data, BarrierCounter,
};
use rt_app_rs::engine::calibration::{calibrate_cpu_cycles, CpuBurnMode, NsPerLoop};
use rt_app_rs::engine::{create_thread, shutdown, EngineState, ThreadHandle};
use rt_app_rs::types::{AppOptions, ThreadIndex};
use rt_app_rs::utils::timespec_to_nsec;

// ---------------------------------------------------------------------------
// Exit codes
// ---------------------------------------------------------------------------

const EXIT_SUCCESS: u8 = 0;
const EXIT_FAILURE: u8 = 1;
const EXIT_INVALID_CONFIG: u8 = 2;

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

fn main() -> ExitCode {
    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let cli = Cli::parse();

    // Handle --print-template early exit
    if cli.print_template {
        cli.print_template_and_exit();
        return ExitCode::from(EXIT_SUCCESS);
    }

    match run(&cli) {
        Ok(()) => ExitCode::from(EXIT_SUCCESS),
        Err(e) => {
            error!("{e}");
            match e {
                AppError::Config(_) => ExitCode::from(EXIT_INVALID_CONFIG),
                _ => ExitCode::from(EXIT_FAILURE),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Application error type
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
enum AppError {
    #[error("configuration error: {0}")]
    Config(#[from] ConfigError),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("calibration failed: could not determine ns_per_loop")]
    CalibrationFailed,

    #[error("no tasks defined in configuration")]
    NoTasks,
}

// ---------------------------------------------------------------------------
// Main run function
// ---------------------------------------------------------------------------

fn run(cli: &Cli) -> Result<(), AppError> {
    // Step 1: Parse configuration
    let config = match cli.config_source() {
        Some(ConfigSource::File(path)) => {
            info!("loading configuration from {}", path.display());
            parse_config(Path::new(&path))?
        }
        Some(ConfigSource::Stdin) => {
            info!("loading configuration from stdin");
            parse_config_stdin()?
        }
        None => {
            return Err(AppError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "no configuration file specified",
            )));
        }
    };

    // Step 2: Validate configuration
    if config.tasks.is_empty() {
        return Err(AppError::NoTasks);
    }

    // Count active tasks (those with instance > 0)
    let active_tasks: Vec<_> = config
        .tasks
        .iter()
        .filter(|t| t.num_instances > 0)
        .collect();

    if active_tasks.is_empty() {
        warn!("no active tasks (all have instance=0)");
    }

    let total_threads: u32 = active_tasks.iter().map(|t| t.num_instances).sum();
    info!(
        "configuration loaded: {} tasks, {} threads",
        config.tasks.len(),
        total_threads
    );

    // Step 3: Convert config to runtime types
    let opts = AppOptions::from(&config.global);

    // Step 4: Determine CPU burn mode
    let cpu_burn_mode = if opts.precise_mode {
        info!("using precise mode (clock_gettime spin-wait)");
        CpuBurnMode::Precise
    } else {
        let ns_per_loop = run_calibration(&config, &opts)?;
        info!("calibration: {} ns/loop", ns_per_loop.0);
        CpuBurnMode::Calibrated(ns_per_loop)
    };

    // Step 5: Build engine state
    let state = build_engine_state(&config, opts, cpu_burn_mode)?;
    let state = Arc::new(state);

    // Step 6: Install signal handlers
    install_signal_handlers(&state);

    // Step 7: Spawn worker threads
    let log_capacity = log_capacity_from_config(&config.global);
    let mut handles = spawn_threads(&config, &state, log_capacity)?;

    // Step 8: Wait for completion
    wait_for_completion(&config, &state);

    // Step 9: Shutdown
    info!("shutting down...");
    shutdown(&state, &mut handles);
    info!("all threads joined, exiting");

    // Check if any thread failed (e.g., scheduling error).
    // Return error to exit with non-zero code (matching C behavior).
    if state.thread_failed.load(Ordering::Relaxed) {
        return Err(AppError::Io(std::io::Error::other("thread failed")));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Calibration
// ---------------------------------------------------------------------------

fn run_calibration(_config: &RtAppConfig, opts: &AppOptions) -> Result<NsPerLoop, AppError> {
    // If ns_per_loop is provided directly, use it
    if let Some(ns) = opts.calib_ns_per_loop {
        info!("using provided calibration: {} ns/loop", ns);
        return Ok(NsPerLoop(ns as u32));
    }

    // Run calibration
    info!("running CPU calibration (this may take a few seconds)...");

    // Pin to the requested CPU during calibration (matching C behavior).
    // This ensures calibration measures the correct CPU's performance.
    let orig_affinity = if opts.calib_cpu >= 0 {
        use nix::sched::{sched_getaffinity, sched_setaffinity, CpuSet};
        use nix::unistd::Pid;

        debug!("pinning to CPU {} for calibration", opts.calib_cpu);

        // Save original affinity
        let orig = sched_getaffinity(Pid::from_raw(0)).ok();

        // Set affinity to calibration CPU
        let mut calib_set = CpuSet::new();
        if calib_set.set(opts.calib_cpu as usize).is_ok() {
            if let Err(e) = sched_setaffinity(Pid::from_raw(0), &calib_set) {
                warn!("failed to pin to CPU {}: {}", opts.calib_cpu, e);
            }
        }
        orig
    } else {
        None
    };

    let result = calibrate_cpu_cycles(ClockId::CLOCK_MONOTONIC);

    // Restore original affinity
    if let Some(orig) = orig_affinity {
        use nix::sched::sched_setaffinity;
        use nix::unistd::Pid;
        let _ = sched_setaffinity(Pid::from_raw(0), &orig);
    }

    result.ok_or(AppError::CalibrationFailed)
}

// ---------------------------------------------------------------------------
// Engine state construction
// ---------------------------------------------------------------------------

fn build_engine_state(
    config: &RtAppConfig,
    opts: AppOptions,
    cpu_burn_mode: CpuBurnMode,
) -> Result<EngineState, AppError> {
    // Build barrier counter to determine barrier participant counts
    let barrier_counter = BarrierCounter::from_tasks(&config.tasks, &config.resources);

    // Build resource handles
    let mem_buffer_size = opts.mem_buffer_size.unwrap_or(4 * 1024 * 1024);
    let resources = build_resource_handles(&config.resources, &barrier_counter, mem_buffer_size);

    // Get application start time
    let t_start = clock_gettime(ClockId::CLOCK_MONOTONIC)
        .map_err(|e| std::io::Error::other(format!("clock_gettime: {e}")))?;
    let t_start_ns = timespec_to_nsec(&t_start);

    Ok(EngineState {
        continue_running: AtomicBool::new(true),
        thread_failed: AtomicBool::new(false),
        running_threads: AtomicI32::new(0),
        cpu_burn_mode,
        resources,
        opts,
        t_zero_ns: Mutex::new(None),
        t_start_ns,
    })
}

// ---------------------------------------------------------------------------
// Signal handling
// ---------------------------------------------------------------------------

/// Global flag for signal-hook to set. We use a static AtomicBool because
/// signal handlers must be able to access the flag without holding any locks.
static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

fn install_signal_handlers(state: &Arc<EngineState>) {
    // Use signal-hook's flag registration which is designed for this use case
    // This registers the flag to be set when any of these signals arrive
    let signals = [
        signal_hook::consts::SIGINT,
        signal_hook::consts::SIGTERM,
        signal_hook::consts::SIGHUP,
        signal_hook::consts::SIGQUIT,
    ];

    // Spawn a monitoring thread that checks for signals and updates engine state
    let state_clone = Arc::clone(state);
    thread::spawn(move || {
        while state_clone.continue_running.load(Ordering::Relaxed) {
            if SHUTDOWN_REQUESTED.load(Ordering::Relaxed) {
                info!("shutdown signal received");
                state_clone.continue_running.store(false, Ordering::SeqCst);
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
    });

    // Register signals to set our global flag
    for sig in signals {
        // Use unsafe low_level::register to set our static flag
        // SAFETY: We're only setting an AtomicBool which is async-signal-safe
        if let Err(e) = unsafe {
            signal_hook::low_level::register(sig, || {
                SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
            })
        } {
            warn!(
                "failed to register signal handler for signal {}: {}",
                sig, e
            );
        }
    }

    debug!("signal handlers installed for SIGINT, SIGTERM, SIGHUP, SIGQUIT");
}

// ---------------------------------------------------------------------------
// Thread spawning
// ---------------------------------------------------------------------------

fn spawn_threads(
    config: &RtAppConfig,
    state: &Arc<EngineState>,
    log_capacity: usize,
) -> Result<Vec<ThreadHandle>, AppError> {
    // Count total threads for the startup barrier
    let total_threads: usize = config
        .tasks
        .iter()
        .filter(|t| t.num_instances > 0)
        .map(|t| t.num_instances as usize)
        .sum();

    if total_threads == 0 {
        return Ok(Vec::new());
    }

    // Create startup barrier (all threads + main thread)
    let startup_barrier = Arc::new(Barrier::new(total_threads + 1));

    let mut handles = Vec::with_capacity(total_threads);
    let mut thread_idx = 0usize;

    for task in &config.tasks {
        // Skip tasks with instance=0 (they are forked, not spawned at startup)
        if task.num_instances == 0 {
            debug!("skipping task '{}' (instance=0, forked only)", task.name);
            continue;
        }

        for instance in 0..task.num_instances {
            let tdata = task_config_to_thread_data(task, &config.global, ThreadIndex(thread_idx));

            let handle = create_thread(
                &tdata,
                ThreadIndex(thread_idx),
                false, // not forked
                instance,
                Some(Arc::clone(&startup_barrier)),
                Arc::clone(state),
                log_capacity,
            )?;

            handles.push(handle);
            thread_idx += 1;
        }
    }

    // Wait at the startup barrier for all threads to be ready
    info!("waiting for {} threads to start...", total_threads);
    startup_barrier.wait();
    info!("all threads started");

    state
        .running_threads
        .store(total_threads as i32, Ordering::SeqCst);

    Ok(handles)
}

// ---------------------------------------------------------------------------
// Wait for completion
// ---------------------------------------------------------------------------

fn wait_for_completion(config: &RtAppConfig, state: &Arc<EngineState>) {
    // If a duration is set, wait for that duration
    if let Some(duration) = state.opts.duration {
        info!("running for {} seconds...", duration.as_secs());

        let check_interval = Duration::from_millis(100);
        let mut elapsed = Duration::ZERO;

        while elapsed < duration && state.continue_running.load(Ordering::Relaxed) {
            thread::sleep(check_interval);
            elapsed += check_interval;
        }

        if elapsed >= duration {
            info!("duration elapsed, signaling shutdown");
            state.continue_running.store(false, Ordering::SeqCst);
        }
    } else {
        // No duration: wait until all threads finish or signal received
        info!("running until threads complete (no duration limit)");

        // Check periodically if threads are still running
        // In the full implementation, we'd track thread completion
        // For now, we just wait for signals
        while state.continue_running.load(Ordering::Relaxed)
            && state.running_threads.load(Ordering::Relaxed) > 0
        {
            thread::sleep(Duration::from_millis(100));
        }
    }

    let _ = config; // Silence unused warning
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rt_app_rs::config::parse_config_str;

    #[test]
    fn parse_minimal_config_succeeds() {
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
    }

    #[test]
    fn app_options_from_config() {
        let json = r#"{
            "global": {
                "duration": 5,
                "calibration": 100,
                "gnuplot": true
            },
            "tasks": {
                "t1": { "run": 1000, "sleep": 1000 }
            }
        }"#;
        let config = parse_config_str(json).unwrap();
        let opts = AppOptions::from(&config.global);

        assert_eq!(opts.duration, Some(Duration::from_secs(5)));
        assert_eq!(opts.calib_ns_per_loop, Some(100));
        assert!(opts.gnuplot);
    }
}
