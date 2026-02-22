//! rt-app-fuzzer: Stress-testing tool for comparing C rt-app and rt-app-rs.
//!
//! Generates random valid JSON workloads and runs them through both
//! implementations to detect behavioral differences.
//!
//! The C rt-app must be built from the rt-app-orig submodule. If not found,
//! use --build to automatically build it.

mod comparator;
mod generator;
mod runner;

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

use clap::Parser;

use comparator::{Comparator, ComparatorConfig, ComparisonOutcome};
use generator::{GeneratorConfig, SafetyLimits, WorkloadGenerator};
use runner::{RunnerConfig, WorkloadRunner};

/// Path to the C rt-app binary relative to the bug_finding directory.
const C_RTAPP_SUBMODULE_PATH: &str = "../rt-app-orig/src/rt-app";

/// Path to the rt-app-orig submodule directory.
const SUBMODULE_DIR: &str = "../rt-app-orig";

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

/// Stress-testing / fuzzer tool for comparing C rt-app and rt-app-rs.
#[derive(Parser, Debug)]
#[command(name = "rt-app-fuzzer")]
#[command(about = "Generates random workloads and compares C rt-app vs rt-app-rs")]
struct Cli {
    /// Number of random configs to test.
    #[arg(long, default_value = "10")]
    iterations: u32,

    /// Random seed for reproducibility.
    #[arg(long)]
    seed: Option<u64>,

    /// Maximum threads per workload.
    #[arg(long, default_value = "4")]
    max_threads: u32,

    /// Maximum workload duration in milliseconds.
    #[arg(long, default_value = "2000")]
    max_duration_ms: u32,

    /// Enable rt-app-rs-only features (future use).
    #[arg(long)]
    extended: bool,

    /// Save failing configs to bug_finding/failures/.
    #[arg(long)]
    keep_failures: bool,

    /// Print each generated config.
    #[arg(long)]
    verbose: bool,

    /// Build C rt-app from submodule if not already built.
    #[arg(long)]
    build: bool,

    /// Path to rt-app-rs Cargo.toml.
    #[arg(long, default_value = "../Cargo.toml")]
    rt_app_rs_path: PathBuf,
}

// ---------------------------------------------------------------------------
// Statistics tracking
// ---------------------------------------------------------------------------

/// Accumulated statistics from fuzzing runs.
#[derive(Debug, Default)]
struct FuzzStats {
    /// Total number of iterations run.
    total: u32,
    /// Number of consistent results.
    consistent: u32,
    /// Number of divergent results.
    divergent: u32,
    /// Number of Rust failures.
    rs_failures: u32,
    /// Number of C failures.
    c_failures: u32,
}

impl FuzzStats {
    fn record_outcome(&mut self, outcome: &ComparisonOutcome, c_failed: bool, rs_failed: bool) {
        self.total += 1;
        match outcome {
            ComparisonOutcome::Consistent => self.consistent += 1,
            ComparisonOutcome::CNotAvailable => {
                // This should not happen since we require C rt-app
                panic!("C rt-app unavailable but it should have been checked at startup");
            }
            ComparisonOutcome::Divergent(_) => self.divergent += 1,
        }
        if c_failed {
            self.c_failures += 1;
        }
        if rs_failed {
            self.rs_failures += 1;
        }
    }

    fn print_summary(&self) {
        println!("\n=== Fuzzing Summary ===");
        println!("Total iterations: {}", self.total);
        println!("Consistent:       {}", self.consistent);
        println!("Divergent:        {}", self.divergent);
        println!("C failures:       {}", self.c_failures);
        println!("Rust failures:    {}", self.rs_failures);

        if self.divergent > 0 {
            println!("\n[WARNING] {} divergences found!", self.divergent);
        } else {
            println!("\n[OK] All comparisons were consistent.");
        }
    }
}

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

fn main() {
    let cli = Cli::parse();

    let c_rtapp_path = PathBuf::from(C_RTAPP_SUBMODULE_PATH);

    // Build C rt-app from submodule if requested
    if cli.build {
        build_c_rtapp();
    }

    // Check that C rt-app is available (required)
    if !c_rtapp_path.exists() {
        eprintln!("Error: C rt-app not found at {}", c_rtapp_path.display());
        eprintln!();
        eprintln!("The fuzzer requires the C rt-app from the rt-app-orig submodule.");
        eprintln!("To build it, run:");
        eprintln!("  rt-app-fuzzer --build --iterations 0");
        eprintln!("Or manually:");
        eprintln!("  cd ../rt-app-orig && ./autogen.sh && ./configure && make");
        std::process::exit(1);
    }

    // Determine seed
    let seed = cli.seed.unwrap_or_else(|| {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(12345);
        println!("Using random seed: {}", seed);
        seed
    });

    // Build configuration
    let safety_limits = SafetyLimits {
        max_threads: cli.max_threads,
        max_duration_secs: cli.max_duration_ms / 1000,
        ..Default::default()
    };

    let generator_config = GeneratorConfig {
        limits: safety_limits,
        extended: cli.extended,
    };

    let runner_config = RunnerConfig {
        c_rt_app_path: c_rtapp_path.clone(),
        rs_manifest_path: cli.rt_app_rs_path.clone(),
        ..Default::default()
    };

    let comparator_config = ComparatorConfig::default();

    // Create components
    let mut generator = WorkloadGenerator::new(generator_config, seed);
    let runner = WorkloadRunner::new(runner_config);
    let comparator = Comparator::new(comparator_config);

    // Print initial status
    println!("rt-app-fuzzer starting...");
    println!("  Iterations: {}", cli.iterations);
    println!("  Seed: {}", seed);
    println!("  Max threads: {}", cli.max_threads);
    println!("  Max duration: {}ms", cli.max_duration_ms);
    println!("  C rt-app: {}", c_rtapp_path.display());
    println!("  rt-app-rs: {}", cli.rt_app_rs_path.display());
    println!();

    // Create failures directory if needed
    if cli.keep_failures {
        let failures_dir = PathBuf::from("failures");
        if !failures_dir.exists() {
            fs::create_dir_all(&failures_dir).expect("Failed to create failures directory");
        }
    }

    // Run fuzzing iterations
    let mut stats = FuzzStats::default();
    let start = Instant::now();

    for i in 0..cli.iterations {
        print!("Iteration {}/{}... ", i + 1, cli.iterations);

        // Generate workload
        let workload = generator.generate();

        if cli.verbose {
            println!("\nGenerated workload:");
            println!("{}", serde_json::to_string_pretty(&workload).unwrap());
        }

        // Run through both implementations
        let run_result = runner.run(&workload, cli.verbose);

        // Compare results
        let comparison = comparator.compare(&run_result);

        // Track statistics
        stats.record_outcome(
            &comparison.outcome,
            run_result.c_result.failed(),
            run_result.rs_result.failed(),
        );

        // Print result
        match &comparison.outcome {
            ComparisonOutcome::Consistent => {
                println!("OK - {}", comparison.summary);
            }
            ComparisonOutcome::CNotAvailable => {
                // Should not happen - we checked at startup
                unreachable!("C rt-app should be available");
            }
            ComparisonOutcome::Divergent(divergences) => {
                println!("DIVERGENT!");
                for d in divergences {
                    println!("  - {}", d);
                }

                // Save failing config
                if cli.keep_failures {
                    save_failure(i, &run_result.workload_json, &comparison.summary);
                }
            }
        }

        // Print verbose output details
        if cli.verbose {
            print_run_details(&run_result);
        }
    }

    let elapsed = start.elapsed();
    println!("\nCompleted in {:?}", elapsed);

    stats.print_summary();

    // Exit with error if divergences found
    if stats.divergent > 0 {
        std::process::exit(1);
    }
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Build C rt-app from the submodule.
fn build_c_rtapp() {
    let submodule_dir = PathBuf::from(SUBMODULE_DIR);

    if !submodule_dir.exists() {
        eprintln!("Error: rt-app-orig submodule not found at {}", submodule_dir.display());
        eprintln!("Run: git submodule update --init");
        std::process::exit(1);
    }

    println!("Building C rt-app from submodule...");

    // Check if configure exists, if not run autogen.sh
    let configure_path = submodule_dir.join("configure");
    if !configure_path.exists() {
        println!("  Running autogen.sh...");
        let status = Command::new("./autogen.sh")
            .current_dir(&submodule_dir)
            .status()
            .expect("Failed to run autogen.sh");
        if !status.success() {
            eprintln!("autogen.sh failed");
            std::process::exit(1);
        }
    }

    // Check if Makefile exists, if not run configure
    let makefile_path = submodule_dir.join("Makefile");
    if !makefile_path.exists() {
        println!("  Running configure...");
        let status = Command::new("./configure")
            .current_dir(&submodule_dir)
            .status()
            .expect("Failed to run configure");
        if !status.success() {
            eprintln!("configure failed");
            std::process::exit(1);
        }
    }

    // Run make
    println!("  Running make...");
    let status = Command::new("make")
        .current_dir(&submodule_dir)
        .status()
        .expect("Failed to run make");
    if !status.success() {
        eprintln!("make failed");
        std::process::exit(1);
    }

    println!("  C rt-app built successfully.");
}

fn save_failure(iteration: u32, workload_json: &str, summary: &str) {
    let filename = format!("failures/failure_{}.json", iteration);
    let content = format!(
        "// Divergence: {}\n// Iteration: {}\n{}",
        summary, iteration, workload_json
    );

    if let Err(e) = fs::write(&filename, content) {
        eprintln!("Failed to save failure to {}: {}", filename, e);
    } else {
        println!("  Saved to {}", filename);
    }
}

fn print_run_details(result: &runner::WorkloadRunResult) {
    println!("\n  C rt-app:");
    if result.c_result.available {
        println!("    Exit code: {:?}", result.c_result.exit_code);
        println!("    Wall time: {:?}", result.c_result.wall_time);
        println!("    Timed out: {}", result.c_result.timed_out);
        if !result.c_result.stdout.is_empty() {
            println!("    Stdout: {}", truncate(&result.c_result.stdout, 200));
        }
        if !result.c_result.stderr.is_empty() {
            println!("    Stderr: {}", truncate(&result.c_result.stderr, 200));
        }
    } else {
        println!("    Not available");
    }

    println!("\n  rt-app-rs:");
    println!("    Exit code: {:?}", result.rs_result.exit_code);
    println!("    Wall time: {:?}", result.rs_result.wall_time);
    println!("    Timed out: {}", result.rs_result.timed_out);
    if !result.rs_result.stdout.is_empty() {
        println!("    Stdout: {}", truncate(&result.rs_result.stdout, 200));
    }
    if !result.rs_result.stderr.is_empty() {
        println!("    Stderr: {}", truncate(&result.rs_result.stderr, 200));
    }
    println!();
}

fn truncate(s: &str, max_len: usize) -> String {
    let s = s.trim();
    if s.len() <= max_len {
        s.replace('\n', " ")
    } else {
        format!("{}...", &s[..max_len].replace('\n', " "))
    }
}
