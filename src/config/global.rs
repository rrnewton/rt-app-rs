//! Parsing of the `"global"` section of the rt-app JSON configuration.
//!
//! The global section controls application-wide parameters such as duration,
//! default scheduling policy, calibration, logging, and ftrace settings.

use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;

use crate::types::{FtraceLevel, SchedulingPolicy};

use super::ConfigError;

// ---------------------------------------------------------------------------
// Constants (matching C defaults)
// ---------------------------------------------------------------------------

/// Default memory buffer size: 4 MiB.
pub const DEFAULT_MEM_BUF_SIZE: usize = 4 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Calibration
// ---------------------------------------------------------------------------

/// The `calibration` field can be either an integer (ns-per-loop) or a string
/// like `"CPU0"`, `"CPU3"`, etc.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Calibration {
    /// Use the given ns-per-loop value directly (skip calibration).
    NsPerLoop(u64),
    /// Calibrate on the specified CPU number.
    Cpu(u32),
}

impl Default for Calibration {
    fn default() -> Self {
        Self::Cpu(0)
    }
}

impl<'de> Deserialize<'de> for Calibration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de;

        struct CalibrationVisitor;

        impl<'de> de::Visitor<'de> for CalibrationVisitor {
            type Value = Calibration;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("an integer (ns-per-loop) or a string like \"CPU0\"")
            }

            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
                if v < 0 {
                    return Err(E::custom(format!("negative calibration value: {v}")));
                }
                Ok(Calibration::NsPerLoop(v as u64))
            }

            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(Calibration::NsPerLoop(v))
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                parse_cpu_string(v).map_err(E::custom)
            }
        }

        deserializer.deserialize_any(CalibrationVisitor)
    }
}

fn parse_cpu_string(s: &str) -> Result<Calibration, String> {
    if let Some(suffix) = s.strip_prefix("CPU") {
        let cpu: u32 = suffix
            .parse()
            .map_err(|_| format!("invalid calibration CPU string: {s:?}"))?;
        Ok(Calibration::Cpu(cpu))
    } else {
        Err(format!(
            "invalid calibration string: {s:?} (expected \"CPU<N>\")"
        ))
    }
}

// ---------------------------------------------------------------------------
// Log size
// ---------------------------------------------------------------------------

/// The `log_size` field can be an integer (MB), or a string mode.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum LogSize {
    /// Use the file system directly (no in-memory buffer).
    #[default]
    File,
    /// Disable logging entirely.
    Disabled,
    /// Fixed buffer size in bytes (the JSON specifies MB, we store bytes).
    BufferBytes(usize),
}

impl<'de> Deserialize<'de> for LogSize {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de;

        struct LogSizeVisitor;

        impl<'de> de::Visitor<'de> for LogSizeVisitor {
            type Value = LogSize;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter
                    .write_str("an integer (MB) or a string: \"file\", \"disable\", or \"auto\"")
            }

            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
                if v < 0 {
                    return Err(E::custom(format!("negative log_size: {v}")));
                }
                Ok(LogSize::BufferBytes((v as usize) << 20))
            }

            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(LogSize::BufferBytes((v as usize) << 20))
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                match v {
                    "disable" => Ok(LogSize::Disabled),
                    "file" => Ok(LogSize::File),
                    // "auto" not yet implemented, falls back to file mode
                    "auto" => Ok(LogSize::File),
                    _ => Err(E::custom(format!(
                        "invalid log_size string: {v:?} (expected \"file\", \"disable\", or \"auto\")"
                    ))),
                }
            }
        }

        deserializer.deserialize_any(LogSizeVisitor)
    }
}

// ---------------------------------------------------------------------------
// Ftrace config
// ---------------------------------------------------------------------------

/// The `ftrace` field can be a boolean (deprecated) or a comma-separated
/// string of category names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FtraceConfig(pub FtraceLevel);

impl Default for FtraceConfig {
    fn default() -> Self {
        Self(FtraceLevel::NONE)
    }
}

impl<'de> Deserialize<'de> for FtraceConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de;

        struct FtraceVisitor;

        impl<'de> de::Visitor<'de> for FtraceVisitor {
            type Value = FtraceConfig;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(
                    "a boolean (deprecated) or a comma-separated string of ftrace categories",
                )
            }

            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
                if v {
                    // Deprecated: enable all categories
                    Ok(FtraceConfig(FtraceLevel::all()))
                } else {
                    Ok(FtraceConfig(FtraceLevel::NONE))
                }
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                parse_ftrace_categories(v)
                    .map(FtraceConfig)
                    .map_err(E::custom)
            }
        }

        deserializer.deserialize_any(FtraceVisitor)
    }
}

fn parse_ftrace_categories(s: &str) -> Result<FtraceLevel, String> {
    if s.is_empty() || s == "none" {
        return Ok(FtraceLevel::NONE);
    }

    let mut level = FtraceLevel::NONE;
    for cat in s.split(',') {
        let cat = cat.trim();
        let flag = match cat {
            "main" => FtraceLevel::MAIN,
            "task" => FtraceLevel::TASK,
            "loop" => FtraceLevel::LOOP,
            "event" => FtraceLevel::EVENT,
            "stats" => FtraceLevel::STATS,
            "attrs" => FtraceLevel::ATTRS,
            _ => return Err(format!("invalid ftrace category: {cat:?}")),
        };
        level |= flag;
    }
    Ok(level)
}

// ---------------------------------------------------------------------------
// GlobalConfig: the parsed global section
// ---------------------------------------------------------------------------

/// Parsed representation of the `"global"` JSON section.
#[derive(Debug, Clone)]
pub struct GlobalConfig {
    /// Application duration in seconds (-1 means run until all tasks complete).
    pub duration_secs: i32,
    /// Default scheduling policy for all tasks.
    pub default_policy: SchedulingPolicy,
    /// Calibration setting.
    pub calibration: Calibration,
    /// Log size configuration.
    pub log_size: LogSize,
    /// Directory for log files.
    pub logdir: PathBuf,
    /// Base name for log files.
    pub log_basename: String,
    /// Ftrace configuration.
    pub ftrace: FtraceConfig,
    /// Whether to lock pages in memory.
    pub lock_pages: bool,
    /// Whether priority inheritance is enabled for mutexes.
    pub pi_enabled: bool,
    /// Path to the I/O device for iorun events.
    pub io_device: PathBuf,
    /// Size of per-thread memory buffer in bytes.
    pub mem_buffer_size: usize,
    /// Whether slack is accumulated across timer events.
    pub cumulative_slack: bool,
    /// Whether to generate gnuplot output.
    pub gnuplot: bool,
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            duration_secs: -1,
            default_policy: SchedulingPolicy::Other,
            calibration: Calibration::default(),
            log_size: LogSize::default(),
            logdir: PathBuf::from("./"),
            log_basename: String::from("rt-app"),
            ftrace: FtraceConfig::default(),
            lock_pages: true,
            pi_enabled: false,
            io_device: PathBuf::from("/dev/null"),
            mem_buffer_size: DEFAULT_MEM_BUF_SIZE,
            cumulative_slack: false,
            gnuplot: false,
        }
    }
}

/// Intermediate serde struct matching the JSON `"global"` object layout.
#[derive(Debug, Deserialize)]
struct RawGlobalConfig {
    #[serde(default = "default_duration")]
    duration: i32,
    #[serde(default = "default_policy")]
    default_policy: String,
    #[serde(default)]
    calibration: Option<Calibration>,
    #[serde(default)]
    log_size: Option<LogSize>,
    #[serde(default = "default_logdir")]
    logdir: String,
    #[serde(default = "default_log_basename")]
    log_basename: String,
    #[serde(default)]
    ftrace: Option<FtraceConfig>,
    #[serde(default = "default_true")]
    lock_pages: bool,
    #[serde(default)]
    pi_enabled: bool,
    #[serde(default = "default_io_device")]
    io_device: String,
    #[serde(default = "default_mem_buffer_size")]
    mem_buffer_size: usize,
    #[serde(default)]
    cumulative_slack: bool,
    #[serde(default)]
    gnuplot: bool,
}

fn default_duration() -> i32 {
    -1
}
fn default_policy() -> String {
    "SCHED_OTHER".to_owned()
}
fn default_logdir() -> String {
    "./".to_owned()
}
fn default_log_basename() -> String {
    "rt-app".to_owned()
}
fn default_true() -> bool {
    true
}
fn default_io_device() -> String {
    "/dev/null".to_owned()
}
fn default_mem_buffer_size() -> usize {
    DEFAULT_MEM_BUF_SIZE
}

impl RawGlobalConfig {
    fn into_global_config(self) -> Result<GlobalConfig, ConfigError> {
        let default_policy: SchedulingPolicy = self
            .default_policy
            .parse()
            .map_err(|_| ConfigError::InvalidPolicy(self.default_policy.clone()))?;

        Ok(GlobalConfig {
            duration_secs: self.duration,
            default_policy,
            calibration: self.calibration.unwrap_or_default(),
            log_size: self.log_size.unwrap_or_default(),
            logdir: PathBuf::from(self.logdir),
            log_basename: self.log_basename,
            ftrace: self.ftrace.unwrap_or_default(),
            lock_pages: self.lock_pages,
            pi_enabled: self.pi_enabled,
            io_device: PathBuf::from(self.io_device),
            mem_buffer_size: self.mem_buffer_size,
            cumulative_slack: self.cumulative_slack,
            gnuplot: self.gnuplot,
        })
    }
}

/// Parse a `GlobalConfig` from a JSON value. Returns defaults if `None`.
pub fn parse_global(value: Option<&serde_json::Value>) -> Result<GlobalConfig, ConfigError> {
    match value {
        Some(v) => {
            let raw: RawGlobalConfig =
                serde_json::from_value(v.clone()).map_err(ConfigError::Json)?;
            raw.into_global_config()
        }
        None => Ok(GlobalConfig::default()),
    }
}

/// Convert a `GlobalConfig` to the runtime `Duration` option.
pub fn global_duration(g: &GlobalConfig) -> Option<Duration> {
    if g.duration_secs < 0 {
        None
    } else {
        Some(Duration::from_secs(g.duration_secs as u64))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_global_config() {
        let g = GlobalConfig::default();
        assert_eq!(g.duration_secs, -1);
        assert_eq!(g.default_policy, SchedulingPolicy::Other);
        assert!(matches!(g.calibration, Calibration::Cpu(0)));
        assert!(matches!(g.log_size, LogSize::File));
        assert!(g.lock_pages);
        assert!(!g.pi_enabled);
        assert_eq!(g.mem_buffer_size, DEFAULT_MEM_BUF_SIZE);
        assert!(!g.cumulative_slack);
        assert!(!g.gnuplot);
    }

    #[test]
    fn parse_global_none_gives_defaults() {
        let g = parse_global(None).unwrap();
        assert_eq!(g.duration_secs, -1);
    }

    #[test]
    fn parse_global_full() {
        let json = serde_json::json!({
            "duration": 5,
            "calibration": "CPU2",
            "default_policy": "SCHED_FIFO",
            "pi_enabled": true,
            "lock_pages": false,
            "logdir": "/tmp/logs",
            "log_basename": "test",
            "ftrace": "main,task",
            "gnuplot": true,
            "io_device": "/dev/zero",
            "mem_buffer_size": 1048576,
            "cumulative_slack": true
        });
        let g = parse_global(Some(&json)).unwrap();
        assert_eq!(g.duration_secs, 5);
        assert_eq!(g.default_policy, SchedulingPolicy::Fifo);
        assert_eq!(g.calibration, Calibration::Cpu(2));
        assert!(!g.lock_pages);
        assert!(g.pi_enabled);
        assert!(g.gnuplot);
        assert!(g.cumulative_slack);
        assert_eq!(g.mem_buffer_size, 1048576);
        assert_eq!(g.ftrace.0, FtraceLevel::MAIN | FtraceLevel::TASK);
    }

    #[test]
    fn calibration_integer() {
        let json = serde_json::json!({"calibration": 500});
        let g = parse_global(Some(&json)).unwrap();
        assert_eq!(g.calibration, Calibration::NsPerLoop(500));
    }

    #[test]
    fn calibration_string() {
        let json = serde_json::json!({"calibration": "CPU3"});
        let g = parse_global(Some(&json)).unwrap();
        assert_eq!(g.calibration, Calibration::Cpu(3));
    }

    #[test]
    fn log_size_integer() {
        let json = serde_json::json!({"log_size": 2});
        let g = parse_global(Some(&json)).unwrap();
        assert_eq!(g.log_size, LogSize::BufferBytes(2 << 20));
    }

    #[test]
    fn log_size_disable() {
        let json = serde_json::json!({"log_size": "disable"});
        let g = parse_global(Some(&json)).unwrap();
        assert_eq!(g.log_size, LogSize::Disabled);
    }

    #[test]
    fn log_size_file() {
        let json = serde_json::json!({"log_size": "file"});
        let g = parse_global(Some(&json)).unwrap();
        assert_eq!(g.log_size, LogSize::File);
    }

    #[test]
    fn log_size_auto_fallback_to_file() {
        let json = serde_json::json!({"log_size": "auto"});
        let g = parse_global(Some(&json)).unwrap();
        assert_eq!(g.log_size, LogSize::File);
    }

    #[test]
    fn ftrace_boolean_deprecated() {
        let json = serde_json::json!({"ftrace": true});
        let g = parse_global(Some(&json)).unwrap();
        assert_eq!(g.ftrace.0, FtraceLevel::all());
    }

    #[test]
    fn ftrace_string() {
        let json = serde_json::json!({"ftrace": "main,loop,event"});
        let g = parse_global(Some(&json)).unwrap();
        assert_eq!(
            g.ftrace.0,
            FtraceLevel::MAIN | FtraceLevel::LOOP | FtraceLevel::EVENT
        );
    }

    #[test]
    fn ftrace_none_string() {
        let json = serde_json::json!({"ftrace": "none"});
        let g = parse_global(Some(&json)).unwrap();
        assert_eq!(g.ftrace.0, FtraceLevel::NONE);
    }

    #[test]
    fn invalid_policy() {
        let json = serde_json::json!({"default_policy": "SCHED_MAGIC"});
        let err = parse_global(Some(&json));
        assert!(err.is_err());
    }

    #[test]
    fn global_duration_negative() {
        let g = GlobalConfig::default();
        assert!(global_duration(&g).is_none());
    }

    #[test]
    fn global_duration_positive() {
        let mut g = GlobalConfig::default();
        g.duration_secs = 10;
        assert_eq!(global_duration(&g), Some(Duration::from_secs(10)));
    }
}
