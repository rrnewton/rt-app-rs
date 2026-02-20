//! CLI argument parsing for rt-app-rs.
//!
//! Ported from the C implementation in `rt-app_args.c` / `rt-app_args.h`.
//! Uses clap derive API for idiomatic Rust argument parsing.

use std::path::PathBuf;

use clap::Parser;

/// Default log level matching the C implementation's default of 50 (NOTICE).
const DEFAULT_LOG_LEVEL: u32 = 50;

/// Version string matching the C format: "PACKAGE VERSION".
/// The C code uses autoconf-generated PACKAGE and VERSION macros;
/// here we use Cargo's built-in package metadata.
/// Constructed at compile time via `concat!` so it is a `&'static str`.
const VERSION_STRING: &str = concat!(env!("CARGO_PKG_NAME"), " ", env!("CARGO_PKG_VERSION"));

/// Exit codes matching the C implementation in `rt-app_types.h`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ExitCode {
    Success = 0,
    Failure = 1,
    InvalidConfig = 2,
    InvalidCommandLine = 3,
}

impl From<ExitCode> for std::process::ExitCode {
    fn from(code: ExitCode) -> Self {
        std::process::ExitCode::from(code as u8)
    }
}

/// Where the JSON configuration comes from: a file path or standard input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigSource {
    /// Read configuration from the given file path.
    File(PathBuf),
    /// Read configuration from standard input (specified as "-" on the CLI).
    Stdin,
}

/// Log verbosity level for rt-app-rs.
///
/// Wraps a `u32` matching the C implementation's numeric levels:
///   10 = ERROR/CRITICAL, 50 = NOTICE (default), 75 = INFO, 100 = DEBUG.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LogLevel(u32);

impl LogLevel {
    /// The raw numeric level value.
    pub fn value(self) -> u32 {
        self.0
    }
}

impl Default for LogLevel {
    fn default() -> Self {
        Self(DEFAULT_LOG_LEVEL)
    }
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for LogLevel {
    type Err = std::num::ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<u32>().map(Self)
    }
}

/// rt-app-rs: a real-time workload simulator.
///
/// Reads a JSON task-set description and generates the corresponding
/// real-time workload. The config can be read from a file or from stdin
/// (by passing "-" as the config path).
#[derive(Parser, Debug)]
#[command(
    name = "rt-app-rs",
    version = VERSION_STRING,
    about = "Real-time workload simulator (Rust port of rt-app)"
)]
pub struct Cli {
    /// Set verbosity level (10: ERROR/CRITICAL, 50: NOTICE (default),
    /// 75: INFO, 100: DEBUG).
    #[arg(short = 'l', long = "log-level", default_value_t = LogLevel::default())]
    pub log_level: LogLevel,

    /// Path to a JSON task-set description file, or "-" to read from stdin.
    #[arg(value_name = "CONFIG")]
    config: String,
}

impl Cli {
    /// Parse the positional config argument into a typed [`ConfigSource`].
    pub fn config_source(&self) -> ConfigSource {
        if self.config == "-" {
            ConfigSource::Stdin
        } else {
            ConfigSource::File(PathBuf::from(&self.config))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: parse CLI args from an iterator, returning the `Cli` struct.
    fn parse(args: &[&str]) -> Cli {
        Cli::parse_from(args)
    }

    #[test]
    fn positional_config_file() {
        let cli = parse(&["rt-app-rs", "taskset.json"]);
        assert_eq!(
            cli.config_source(),
            ConfigSource::File(PathBuf::from("taskset.json"))
        );
    }

    #[test]
    fn positional_config_stdin() {
        let cli = parse(&["rt-app-rs", "-"]);
        assert_eq!(cli.config_source(), ConfigSource::Stdin);
    }

    #[test]
    fn default_log_level() {
        let cli = parse(&["rt-app-rs", "taskset.json"]);
        assert_eq!(cli.log_level.value(), DEFAULT_LOG_LEVEL);
    }

    #[test]
    fn custom_log_level_short() {
        let cli = parse(&["rt-app-rs", "-l", "100", "taskset.json"]);
        assert_eq!(cli.log_level.value(), 100);
    }

    #[test]
    fn custom_log_level_long() {
        let cli = parse(&["rt-app-rs", "--log-level", "10", "taskset.json"]);
        assert_eq!(cli.log_level.value(), 10);
    }

    #[test]
    fn exit_code_values() {
        assert_eq!(ExitCode::Success as u8, 0);
        assert_eq!(ExitCode::Failure as u8, 1);
        assert_eq!(ExitCode::InvalidConfig as u8, 2);
        assert_eq!(ExitCode::InvalidCommandLine as u8, 3);
    }

    #[test]
    fn exit_code_into_process_exit_code() {
        // Verify the From conversion compiles and runs.
        let _: std::process::ExitCode = ExitCode::Success.into();
        let _: std::process::ExitCode = ExitCode::InvalidCommandLine.into();
    }

    #[test]
    fn version_string_format() {
        assert!(VERSION_STRING.starts_with("rt-app-rs "));
        assert!(VERSION_STRING.contains(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn log_level_ordering() {
        let error = LogLevel(10);
        let notice = LogLevel(50);
        let debug = LogLevel(100);
        assert!(error < notice);
        assert!(notice < debug);
    }

    #[test]
    fn log_level_display() {
        assert_eq!(LogLevel(75).to_string(), "75");
    }

    #[test]
    fn log_level_parse() {
        let level: LogLevel = "42".parse().unwrap();
        assert_eq!(level.value(), 42);
    }

    #[test]
    fn log_level_parse_invalid() {
        assert!("abc".parse::<LogLevel>().is_err());
    }

    #[test]
    fn missing_config_arg_errors() {
        // clap should error when no positional arg is provided.
        let result = Cli::try_parse_from(["rt-app-rs"]);
        assert!(result.is_err());
    }
}
