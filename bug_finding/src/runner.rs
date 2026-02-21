//! Runs workloads through both rt-app implementations.
//!
//! Executes generated JSON workloads through both the C rt-app and rt-app-rs,
//! capturing output and timing information for comparison.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;
use tempfile::NamedTempFile;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Result of running a single rt-app implementation.
#[derive(Debug)]
pub struct RunResult {
    /// Whether the implementation was available to run.
    pub available: bool,
    /// Exit code (None if timed out or unavailable).
    pub exit_code: Option<i32>,
    /// Standard output.
    pub stdout: String,
    /// Standard error.
    pub stderr: String,
    /// Wall-clock time for the run.
    pub wall_time: Duration,
    /// Whether the run timed out.
    pub timed_out: bool,
}

impl RunResult {
    fn unavailable() -> Self {
        Self {
            available: false,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            wall_time: Duration::ZERO,
            timed_out: false,
        }
    }

    fn from_output(output: Output, wall_time: Duration) -> Self {
        Self {
            available: true,
            exit_code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            wall_time,
            timed_out: false,
        }
    }

    fn timed_out(wall_time: Duration) -> Self {
        Self {
            available: true,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            wall_time,
            timed_out: true,
        }
    }

    /// Check if the run succeeded (exit code 0).
    pub fn succeeded(&self) -> bool {
        self.available && self.exit_code == Some(0) && !self.timed_out
    }

    /// Check if the run failed (non-zero exit or timeout).
    pub fn failed(&self) -> bool {
        self.available && (self.exit_code != Some(0) || self.timed_out)
    }
}

/// Results from running a workload through both implementations.
#[derive(Debug)]
pub struct WorkloadRunResult {
    /// The JSON workload that was run.
    pub workload_json: String,
    /// Result from C rt-app.
    pub c_result: RunResult,
    /// Result from rt-app-rs.
    pub rs_result: RunResult,
}

// ---------------------------------------------------------------------------
// Runner configuration
// ---------------------------------------------------------------------------

/// Configuration for the workload runner.
#[derive(Debug, Clone)]
pub struct RunnerConfig {
    /// Path to the C rt-app binary.
    pub c_rt_app_path: PathBuf,
    /// Path to the rt-app-rs manifest (Cargo.toml).
    pub rs_manifest_path: PathBuf,
    /// Timeout multiplier (timeout = duration * multiplier).
    pub timeout_multiplier: f64,
    /// Minimum timeout in seconds.
    pub min_timeout_secs: u64,
}

impl Default for RunnerConfig {
    fn default() -> Self {
        Self {
            c_rt_app_path: PathBuf::from("rt-app-orig/rt-app"),
            rs_manifest_path: PathBuf::from("../Cargo.toml"),
            timeout_multiplier: 2.0,
            min_timeout_secs: 5,
        }
    }
}

// ---------------------------------------------------------------------------
// Workload runner
// ---------------------------------------------------------------------------

/// Runs workloads through both rt-app implementations.
pub struct WorkloadRunner {
    config: RunnerConfig,
}

impl WorkloadRunner {
    /// Create a new runner with the given configuration.
    pub fn new(config: RunnerConfig) -> Self {
        Self { config }
    }

    /// Check if the C rt-app binary is available.
    pub fn c_rt_app_available(&self) -> bool {
        self.config.c_rt_app_path.exists()
    }

    /// Run a workload through both implementations.
    pub fn run(&self, workload: &Value, verbose: bool) -> WorkloadRunResult {
        let workload_json =
            serde_json::to_string_pretty(workload).expect("Failed to serialize workload");

        // Extract duration from workload for timeout calculation
        let duration_secs = extract_duration(workload);
        let timeout = self.calculate_timeout(duration_secs);

        if verbose {
            eprintln!(
                "Running workload with duration {}s, timeout {:?}",
                duration_secs, timeout
            );
        }

        // Write workload to temp file
        let temp_file = self.write_temp_file(&workload_json);
        let temp_path = temp_file.path();

        // Run C rt-app
        let c_result = if self.c_rt_app_available() {
            if verbose {
                eprintln!(
                    "Running C rt-app: {} {}",
                    self.config.c_rt_app_path.display(),
                    temp_path.display()
                );
            }
            self.run_c_rt_app(temp_path, timeout)
        } else {
            if verbose {
                eprintln!(
                    "C rt-app not available at {}",
                    self.config.c_rt_app_path.display()
                );
            }
            RunResult::unavailable()
        };

        // Run rt-app-rs
        if verbose {
            eprintln!(
                "Running rt-app-rs with manifest: {}",
                self.config.rs_manifest_path.display()
            );
        }
        let rs_result = self.run_rs_rt_app(temp_path, timeout);

        WorkloadRunResult {
            workload_json,
            c_result,
            rs_result,
        }
    }

    fn calculate_timeout(&self, duration_secs: u64) -> Duration {
        let timeout_secs = ((duration_secs as f64) * self.config.timeout_multiplier) as u64;
        let timeout_secs = timeout_secs.max(self.config.min_timeout_secs);
        Duration::from_secs(timeout_secs)
    }

    fn write_temp_file(&self, content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().expect("Failed to create temp file");
        file.write_all(content.as_bytes())
            .expect("Failed to write to temp file");
        file.flush().expect("Failed to flush temp file");
        file
    }

    fn run_c_rt_app(&self, config_path: &Path, timeout: Duration) -> RunResult {
        self.run_command(&self.config.c_rt_app_path, &[config_path], timeout)
    }

    fn run_rs_rt_app(&self, config_path: &Path, timeout: Duration) -> RunResult {
        // Run via cargo run
        let manifest_arg = format!("--manifest-path={}", self.config.rs_manifest_path.display());
        self.run_cargo_command(&manifest_arg, config_path, timeout)
    }

    fn run_command(&self, program: &Path, args: &[&Path], timeout: Duration) -> RunResult {
        let start = Instant::now();

        let result = Command::new(program)
            .args(args.iter().map(|p| p.as_os_str()))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();

        match result {
            Ok(mut child) => {
                // Wait with timeout
                match wait_with_timeout(&mut child, timeout) {
                    WaitResult::Completed(output) => {
                        RunResult::from_output(output, start.elapsed())
                    }
                    WaitResult::TimedOut => {
                        // Kill the process
                        let _ = child.kill();
                        let _ = child.wait();
                        RunResult::timed_out(start.elapsed())
                    }
                }
            }
            Err(e) => RunResult {
                available: false,
                exit_code: None,
                stdout: String::new(),
                stderr: format!("Failed to spawn process: {}", e),
                wall_time: start.elapsed(),
                timed_out: false,
            },
        }
    }

    fn run_cargo_command(
        &self,
        manifest_arg: &str,
        config_path: &Path,
        timeout: Duration,
    ) -> RunResult {
        let start = Instant::now();

        let result = Command::new("cargo")
            .args(["run", "--quiet", manifest_arg, "--"])
            .arg(config_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();

        match result {
            Ok(mut child) => match wait_with_timeout(&mut child, timeout) {
                WaitResult::Completed(output) => RunResult::from_output(output, start.elapsed()),
                WaitResult::TimedOut => {
                    let _ = child.kill();
                    let _ = child.wait();
                    RunResult::timed_out(start.elapsed())
                }
            },
            Err(e) => {
                RunResult {
                    available: true, // cargo should be available
                    exit_code: None,
                    stdout: String::new(),
                    stderr: format!("Failed to run cargo: {}", e),
                    wall_time: start.elapsed(),
                    timed_out: false,
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract the duration from a workload JSON.
fn extract_duration(workload: &Value) -> u64 {
    workload
        .get("global")
        .and_then(|g| g.get("duration"))
        .and_then(|d| d.as_u64())
        .unwrap_or(2)
}

/// Result of waiting for a process.
enum WaitResult {
    Completed(Output),
    TimedOut,
}

/// Wait for a child process with a timeout.
fn wait_with_timeout(child: &mut std::process::Child, timeout: Duration) -> WaitResult {
    let start = Instant::now();
    let poll_interval = Duration::from_millis(100);

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                // Process completed
                let stdout = child
                    .stdout
                    .take()
                    .map(|mut s| {
                        let mut buf = Vec::new();
                        std::io::Read::read_to_end(&mut s, &mut buf).ok();
                        buf
                    })
                    .unwrap_or_default();
                let stderr = child
                    .stderr
                    .take()
                    .map(|mut s| {
                        let mut buf = Vec::new();
                        std::io::Read::read_to_end(&mut s, &mut buf).ok();
                        buf
                    })
                    .unwrap_or_default();

                return WaitResult::Completed(Output {
                    status,
                    stdout,
                    stderr,
                });
            }
            Ok(None) => {
                // Still running
                if start.elapsed() >= timeout {
                    return WaitResult::TimedOut;
                }
                std::thread::sleep(poll_interval);
            }
            Err(_) => {
                // Error checking status
                return WaitResult::TimedOut;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extract_duration_works() {
        let workload = json!({
            "global": {
                "duration": 5
            }
        });
        assert_eq!(extract_duration(&workload), 5);
    }

    #[test]
    fn extract_duration_default() {
        let workload = json!({});
        assert_eq!(extract_duration(&workload), 2);
    }

    #[test]
    fn run_result_succeeded() {
        let result = RunResult {
            available: true,
            exit_code: Some(0),
            stdout: String::new(),
            stderr: String::new(),
            wall_time: Duration::ZERO,
            timed_out: false,
        };
        assert!(result.succeeded());
        assert!(!result.failed());
    }

    #[test]
    fn run_result_failed() {
        let result = RunResult {
            available: true,
            exit_code: Some(1),
            stdout: String::new(),
            stderr: String::new(),
            wall_time: Duration::ZERO,
            timed_out: false,
        };
        assert!(!result.succeeded());
        assert!(result.failed());
    }

    #[test]
    fn run_result_timed_out() {
        let result = RunResult::timed_out(Duration::from_secs(5));
        assert!(!result.succeeded());
        assert!(result.failed());
        assert!(result.timed_out);
    }

    #[test]
    fn run_result_unavailable() {
        let result = RunResult::unavailable();
        assert!(!result.available);
        assert!(!result.succeeded());
        assert!(!result.failed()); // Not failed because not available
    }

    #[test]
    fn calculate_timeout_respects_minimum() {
        let config = RunnerConfig {
            min_timeout_secs: 10,
            timeout_multiplier: 2.0,
            ..Default::default()
        };
        let runner = WorkloadRunner::new(config);

        // 1 second * 2 = 2 seconds, but minimum is 10
        let timeout = runner.calculate_timeout(1);
        assert_eq!(timeout, Duration::from_secs(10));
    }

    #[test]
    fn calculate_timeout_uses_multiplier() {
        let config = RunnerConfig {
            min_timeout_secs: 1,
            timeout_multiplier: 3.0,
            ..Default::default()
        };
        let runner = WorkloadRunner::new(config);

        // 5 seconds * 3 = 15 seconds
        let timeout = runner.calculate_timeout(5);
        assert_eq!(timeout, Duration::from_secs(15));
    }
}
