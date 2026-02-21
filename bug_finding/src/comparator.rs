//! Compares outputs from the C rt-app and rt-app-rs implementations.
//!
//! Analyzes run results to detect behavioral differences between implementations.

use crate::runner::{RunResult, WorkloadRunResult};
use std::time::Duration;

// ---------------------------------------------------------------------------
// Comparison result types
// ---------------------------------------------------------------------------

/// The outcome of comparing two implementations.
#[derive(Debug, Clone, PartialEq)]
pub enum ComparisonOutcome {
    /// Both implementations behaved consistently (both succeeded or both failed).
    Consistent,
    /// Only C rt-app was available, so no comparison was possible.
    CNotAvailable,
    /// The implementations behaved differently.
    Divergent(Vec<Divergence>),
}

impl ComparisonOutcome {
    /// Returns true if the comparison found no issues.
    #[cfg(test)]
    fn is_ok(&self) -> bool {
        matches!(self, Self::Consistent | Self::CNotAvailable)
    }

    /// Returns true if divergences were found.
    #[cfg(test)]
    fn is_divergent(&self) -> bool {
        matches!(self, Self::Divergent(_))
    }
}

/// A specific way in which the implementations diverged.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::enum_variant_names)] // "Mismatch" suffix is descriptive here
pub enum Divergence {
    /// Exit codes differed.
    ExitCodeMismatch {
        c_exit_code: Option<i32>,
        rs_exit_code: Option<i32>,
    },
    /// One succeeded while the other failed.
    SuccessFailureMismatch {
        c_succeeded: bool,
        rs_succeeded: bool,
    },
    /// One timed out while the other didn't.
    TimeoutMismatch {
        c_timed_out: bool,
        rs_timed_out: bool,
    },
    /// Wall-clock times differed significantly.
    WallTimeMismatch {
        c_time: Duration,
        rs_time: Duration,
        difference_ratio: f64,
    },
}

impl std::fmt::Display for Divergence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ExitCodeMismatch {
                c_exit_code,
                rs_exit_code,
            } => {
                write!(
                    f,
                    "Exit code mismatch: C={:?}, Rust={:?}",
                    c_exit_code, rs_exit_code
                )
            }
            Self::SuccessFailureMismatch {
                c_succeeded,
                rs_succeeded,
            } => {
                write!(
                    f,
                    "Success/failure mismatch: C succeeded={}, Rust succeeded={}",
                    c_succeeded, rs_succeeded
                )
            }
            Self::TimeoutMismatch {
                c_timed_out,
                rs_timed_out,
            } => {
                write!(
                    f,
                    "Timeout mismatch: C timed out={}, Rust timed out={}",
                    c_timed_out, rs_timed_out
                )
            }
            Self::WallTimeMismatch {
                c_time,
                rs_time,
                difference_ratio,
            } => {
                write!(
                    f,
                    "Wall time divergence: C={:?}, Rust={:?} (ratio: {:.2}x)",
                    c_time, rs_time, difference_ratio
                )
            }
        }
    }
}

/// Full comparison result with all details.
#[derive(Debug)]
pub struct ComparisonResult {
    /// Overall outcome.
    pub outcome: ComparisonOutcome,
    /// Summary message.
    pub summary: String,
}

impl ComparisonResult {
    /// Create a result indicating C rt-app was not available.
    fn c_not_available() -> Self {
        Self {
            outcome: ComparisonOutcome::CNotAvailable,
            summary: "C rt-app not available, comparison skipped".to_string(),
        }
    }

    /// Create a consistent result.
    fn consistent(summary: String) -> Self {
        Self {
            outcome: ComparisonOutcome::Consistent,
            summary,
        }
    }

    /// Create a divergent result.
    fn divergent(divergences: Vec<Divergence>) -> Self {
        let summary = divergences
            .iter()
            .map(|d| d.to_string())
            .collect::<Vec<_>>()
            .join("; ");
        Self {
            outcome: ComparisonOutcome::Divergent(divergences),
            summary,
        }
    }
}

// ---------------------------------------------------------------------------
// Comparator configuration
// ---------------------------------------------------------------------------

/// Configuration for the comparator.
#[derive(Debug, Clone)]
pub struct ComparatorConfig {
    /// Maximum ratio between wall times before flagging divergence.
    /// A ratio of 1.5 means one can be up to 50% slower/faster.
    pub max_time_ratio: f64,
    /// Minimum wall time difference to consider (avoids noise for short runs).
    pub min_time_diff: Duration,
}

impl Default for ComparatorConfig {
    fn default() -> Self {
        Self {
            max_time_ratio: 1.5,
            min_time_diff: Duration::from_millis(500),
        }
    }
}

// ---------------------------------------------------------------------------
// Comparator
// ---------------------------------------------------------------------------

/// Compares outputs from both rt-app implementations.
pub struct Comparator {
    config: ComparatorConfig,
}

impl Comparator {
    /// Create a new comparator with the given configuration.
    pub fn new(config: ComparatorConfig) -> Self {
        Self { config }
    }

    /// Create a comparator with default configuration.
    #[cfg(test)]
    fn with_defaults() -> Self {
        Self::new(ComparatorConfig::default())
    }

    /// Compare the results of running a workload through both implementations.
    pub fn compare(&self, result: &WorkloadRunResult) -> ComparisonResult {
        // If C rt-app wasn't available, we can't compare
        if !result.c_result.available {
            return ComparisonResult::c_not_available();
        }

        let mut divergences = Vec::new();

        // Check for timeout mismatches
        self.check_timeout_mismatch(&result.c_result, &result.rs_result, &mut divergences);

        // If either timed out, skip further comparison
        if result.c_result.timed_out || result.rs_result.timed_out {
            if divergences.is_empty() {
                return ComparisonResult::consistent("Both timed out".to_string());
            } else {
                return ComparisonResult::divergent(divergences);
            }
        }

        // Check success/failure match
        self.check_success_mismatch(&result.c_result, &result.rs_result, &mut divergences);

        // Check exit code match
        self.check_exit_code_mismatch(&result.c_result, &result.rs_result, &mut divergences);

        // Check wall time divergence (only if both succeeded)
        if result.c_result.succeeded() && result.rs_result.succeeded() {
            self.check_wall_time_divergence(&result.c_result, &result.rs_result, &mut divergences);
        }

        if divergences.is_empty() {
            let summary = if result.c_result.succeeded() {
                format!(
                    "Both succeeded (C: {:?}, Rust: {:?})",
                    result.c_result.wall_time, result.rs_result.wall_time
                )
            } else {
                format!(
                    "Both failed consistently (C: {:?}, Rust: {:?})",
                    result.c_result.exit_code, result.rs_result.exit_code
                )
            };
            ComparisonResult::consistent(summary)
        } else {
            ComparisonResult::divergent(divergences)
        }
    }

    fn check_timeout_mismatch(
        &self,
        c_result: &RunResult,
        rs_result: &RunResult,
        divergences: &mut Vec<Divergence>,
    ) {
        if c_result.timed_out != rs_result.timed_out {
            divergences.push(Divergence::TimeoutMismatch {
                c_timed_out: c_result.timed_out,
                rs_timed_out: rs_result.timed_out,
            });
        }
    }

    fn check_success_mismatch(
        &self,
        c_result: &RunResult,
        rs_result: &RunResult,
        divergences: &mut Vec<Divergence>,
    ) {
        if c_result.succeeded() != rs_result.succeeded() {
            divergences.push(Divergence::SuccessFailureMismatch {
                c_succeeded: c_result.succeeded(),
                rs_succeeded: rs_result.succeeded(),
            });
        }
    }

    fn check_exit_code_mismatch(
        &self,
        c_result: &RunResult,
        rs_result: &RunResult,
        divergences: &mut Vec<Divergence>,
    ) {
        // Only compare if both have exit codes and they differ
        if let (Some(c_code), Some(rs_code)) = (c_result.exit_code, rs_result.exit_code) {
            if c_code != rs_code {
                divergences.push(Divergence::ExitCodeMismatch {
                    c_exit_code: Some(c_code),
                    rs_exit_code: Some(rs_code),
                });
            }
        }
    }

    fn check_wall_time_divergence(
        &self,
        c_result: &RunResult,
        rs_result: &RunResult,
        divergences: &mut Vec<Divergence>,
    ) {
        let c_time = c_result.wall_time;
        let rs_time = rs_result.wall_time;

        // Only check if difference is significant
        let diff = c_time.abs_diff(rs_time);

        if diff < self.config.min_time_diff {
            return;
        }

        // Calculate ratio
        let c_secs = c_time.as_secs_f64();
        let rs_secs = rs_time.as_secs_f64();

        // Avoid division by zero
        if c_secs < 0.001 || rs_secs < 0.001 {
            return;
        }

        let ratio = if c_secs > rs_secs {
            c_secs / rs_secs
        } else {
            rs_secs / c_secs
        };

        if ratio > self.config.max_time_ratio {
            divergences.push(Divergence::WallTimeMismatch {
                c_time,
                rs_time,
                difference_ratio: ratio,
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_success_result(wall_time: Duration) -> RunResult {
        RunResult {
            available: true,
            exit_code: Some(0),
            stdout: String::new(),
            stderr: String::new(),
            wall_time,
            timed_out: false,
        }
    }

    fn make_failure_result(exit_code: i32) -> RunResult {
        RunResult {
            available: true,
            exit_code: Some(exit_code),
            stdout: String::new(),
            stderr: String::new(),
            wall_time: Duration::from_secs(1),
            timed_out: false,
        }
    }

    fn make_timeout_result() -> RunResult {
        RunResult {
            available: true,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            wall_time: Duration::from_secs(10),
            timed_out: true,
        }
    }

    fn make_unavailable_result() -> RunResult {
        RunResult {
            available: false,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            wall_time: Duration::ZERO,
            timed_out: false,
        }
    }

    #[test]
    fn both_succeeded_is_consistent() {
        let comparator = Comparator::with_defaults();
        let result = WorkloadRunResult {
            workload_json: String::new(),
            c_result: make_success_result(Duration::from_secs(1)),
            rs_result: make_success_result(Duration::from_secs(1)),
        };

        let comparison = comparator.compare(&result);
        assert!(comparison.outcome.is_ok());
    }

    #[test]
    fn both_failed_same_code_is_consistent() {
        let comparator = Comparator::with_defaults();
        let result = WorkloadRunResult {
            workload_json: String::new(),
            c_result: make_failure_result(1),
            rs_result: make_failure_result(1),
        };

        let comparison = comparator.compare(&result);
        assert!(comparison.outcome.is_ok());
    }

    #[test]
    fn c_not_available_skips_comparison() {
        let comparator = Comparator::with_defaults();
        let result = WorkloadRunResult {
            workload_json: String::new(),
            c_result: make_unavailable_result(),
            rs_result: make_success_result(Duration::from_secs(1)),
        };

        let comparison = comparator.compare(&result);
        assert_eq!(comparison.outcome, ComparisonOutcome::CNotAvailable);
    }

    #[test]
    fn success_failure_mismatch_detected() {
        let comparator = Comparator::with_defaults();
        let result = WorkloadRunResult {
            workload_json: String::new(),
            c_result: make_success_result(Duration::from_secs(1)),
            rs_result: make_failure_result(1),
        };

        let comparison = comparator.compare(&result);
        assert!(comparison.outcome.is_divergent());
    }

    #[test]
    fn timeout_mismatch_detected() {
        let comparator = Comparator::with_defaults();
        let result = WorkloadRunResult {
            workload_json: String::new(),
            c_result: make_success_result(Duration::from_secs(1)),
            rs_result: make_timeout_result(),
        };

        let comparison = comparator.compare(&result);
        assert!(comparison.outcome.is_divergent());
    }

    #[test]
    fn wall_time_divergence_detected() {
        let config = ComparatorConfig {
            max_time_ratio: 1.5,
            min_time_diff: Duration::from_millis(100),
        };
        let comparator = Comparator::new(config);

        let result = WorkloadRunResult {
            workload_json: String::new(),
            c_result: make_success_result(Duration::from_secs(1)),
            rs_result: make_success_result(Duration::from_secs(3)), // 3x slower
        };

        let comparison = comparator.compare(&result);
        assert!(comparison.outcome.is_divergent());
    }

    #[test]
    fn wall_time_within_tolerance_is_ok() {
        let config = ComparatorConfig {
            max_time_ratio: 1.5,
            min_time_diff: Duration::from_millis(100),
        };
        let comparator = Comparator::new(config);

        let result = WorkloadRunResult {
            workload_json: String::new(),
            c_result: make_success_result(Duration::from_secs(1)),
            rs_result: make_success_result(Duration::from_millis(1200)), // 1.2x slower
        };

        let comparison = comparator.compare(&result);
        assert!(comparison.outcome.is_ok());
    }
}
