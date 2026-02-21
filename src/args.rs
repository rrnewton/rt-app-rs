//! CLI argument parsing for rt-app-rs.
//!
//! Ported from the C implementation in `rt-app_args.c` / `rt-app_args.h`.
//! Uses clap derive API for idiomatic Rust argument parsing.

use std::io::{IsTerminal, Write};
use std::path::PathBuf;

use clap::Parser;
use colored::Colorize;

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

/// The embedded template.json content for `--print-template`.
const TEMPLATE_JSON: &str = include_str!("../doc/examples/template.json");

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

    /// Print the JSON configuration template to stdout and exit.
    #[arg(long = "print-template")]
    pub print_template: bool,

    /// Path to a JSON task-set description file, or "-" to read from stdin.
    #[arg(value_name = "CONFIG", required_unless_present = "print_template")]
    config: Option<String>,
}

impl Cli {
    /// Parse the positional config argument into a typed [`ConfigSource`].
    ///
    /// Returns `None` if no config was provided (only valid when `--print-template` is set).
    pub fn config_source(&self) -> Option<ConfigSource> {
        self.config.as_ref().map(|cfg| {
            if cfg == "-" {
                ConfigSource::Stdin
            } else {
                ConfigSource::File(PathBuf::from(cfg))
            }
        })
    }

    /// Print the embedded template.json to stdout.
    ///
    /// Applies syntax highlighting if stdout is a TTY; otherwise prints raw.
    pub fn print_template_and_exit(&self) {
        let stdout = std::io::stdout();
        let use_color = stdout.is_terminal();

        let output = if use_color {
            colorize_json(TEMPLATE_JSON)
        } else {
            TEMPLATE_JSON.to_string()
        };

        let mut handle = stdout.lock();
        // Ignore write errors (e.g., broken pipe).
        let _ = handle.write_all(output.as_bytes());
    }
}

/// Colorize JSON-with-comments for terminal display.
///
/// Color scheme:
/// - Keys: cyan
/// - String values: green
/// - Numbers: yellow
/// - Keywords (true/false/null): magenta
/// - Comments: bright black (gray)
/// - Punctuation: default (white)
fn colorize_json(input: &str) -> String {
    let mut result = String::with_capacity(input.len() * 2);
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            // Block comment
            '/' if chars.peek() == Some(&'*') => {
                colorize_block_comment(&mut chars, &mut result);
            }
            // Line comment
            '/' if chars.peek() == Some(&'/') => {
                colorize_line_comment(&mut chars, &mut result);
            }
            // String (key or value — determined by context after closing quote)
            '"' => {
                colorize_string(&mut chars, &mut result);
            }
            // Number (digit or leading minus)
            '-' | '0'..='9' => {
                colorize_number(ch, &mut chars, &mut result);
            }
            // Keywords: true, false, null
            't' | 'f' | 'n' => {
                colorize_keyword(ch, &mut chars, &mut result);
            }
            // Punctuation: braces, brackets, colon, comma
            '{' | '}' | '[' | ']' | ':' | ',' => {
                result.push_str(&ch.to_string().white().to_string());
            }
            // Whitespace and other characters
            _ => {
                result.push(ch);
            }
        }
    }
    result
}

/// Colorize a block comment (/* ... */).
fn colorize_block_comment(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    result: &mut String,
) {
    let mut comment = String::from("/*");
    chars.next(); // consume '*'

    while let Some(c) = chars.next() {
        comment.push(c);
        if c == '*' && chars.peek() == Some(&'/') {
            comment.push(chars.next().unwrap());
            break;
        }
    }
    result.push_str(&comment.bright_black().to_string());
}

/// Colorize a line comment (// ...).
fn colorize_line_comment(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    result: &mut String,
) {
    let mut comment = String::from("//");
    chars.next(); // consume second '/'

    for c in chars.by_ref() {
        if c == '\n' {
            result.push_str(&comment.bright_black().to_string());
            result.push('\n');
            return;
        }
        comment.push(c);
    }
    // End of input without newline
    result.push_str(&comment.bright_black().to_string());
}

/// Colorize a JSON string literal.
///
/// Determines whether it's a key (cyan) or value (green) by checking
/// if a colon follows the string (after optional whitespace).
fn colorize_string(chars: &mut std::iter::Peekable<std::str::Chars<'_>>, result: &mut String) {
    let mut content = String::new();

    while let Some(c) = chars.next() {
        if c == '\\' {
            // Escape sequence
            content.push(c);
            if let Some(escaped) = chars.next() {
                content.push(escaped);
            }
        } else if c == '"' {
            break;
        } else {
            content.push(c);
        }
    }

    // Look ahead to see if this is a key (followed by ':')
    let is_key = peek_for_colon(chars);

    let quoted = format!("\"{}\"", content);
    if is_key {
        result.push_str(&quoted.cyan().to_string());
    } else {
        result.push_str(&quoted.green().to_string());
    }
}

/// Check if the next non-whitespace character is a colon.
fn peek_for_colon(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> bool {
    // We need to look ahead without consuming, but Peekable only peeks one.
    // Instead, we'll check immediate next chars stored in the peekable.
    // For simplicity, peek the immediate next char.
    let mut cloned = chars.clone();
    for c in cloned.by_ref() {
        if c.is_whitespace() {
            continue;
        }
        return c == ':';
    }
    false
}

/// Colorize a number literal.
fn colorize_number(
    first: char,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    result: &mut String,
) {
    let mut num = String::from(first);

    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() || c == '.' || c == 'e' || c == 'E' || c == '+' || c == '-' {
            // Handle sign only if it follows 'e' or 'E'
            if (c == '+' || c == '-') && !num.ends_with('e') && !num.ends_with('E') {
                break;
            }
            num.push(chars.next().unwrap());
        } else {
            break;
        }
    }
    result.push_str(&num.yellow().to_string());
}

/// Colorize a keyword (true, false, null).
fn colorize_keyword(
    first: char,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    result: &mut String,
) {
    let mut word = String::from(first);

    while let Some(&c) = chars.peek() {
        if c.is_ascii_alphabetic() {
            word.push(chars.next().unwrap());
        } else {
            break;
        }
    }

    // Only colorize recognized keywords
    match word.as_str() {
        "true" | "false" | "null" => {
            result.push_str(&word.magenta().to_string());
        }
        _ => {
            // Not a keyword, output as-is
            result.push_str(&word);
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
            Some(ConfigSource::File(PathBuf::from("taskset.json")))
        );
    }

    #[test]
    fn positional_config_stdin() {
        let cli = parse(&["rt-app-rs", "-"]);
        assert_eq!(cli.config_source(), Some(ConfigSource::Stdin));
    }

    #[test]
    fn print_template_without_config() {
        // --print-template should work without a config argument
        let cli = parse(&["rt-app-rs", "--print-template"]);
        assert!(cli.print_template);
        assert_eq!(cli.config_source(), None);
    }

    #[test]
    fn print_template_with_config() {
        // --print-template can also be used with a config argument
        let cli = parse(&["rt-app-rs", "--print-template", "taskset.json"]);
        assert!(cli.print_template);
        assert_eq!(
            cli.config_source(),
            Some(ConfigSource::File(PathBuf::from("taskset.json")))
        );
    }

    #[test]
    fn template_contains_expected_sections() {
        // Verify the embedded template contains key sections
        assert!(TEMPLATE_JSON.contains("\"global\""));
        assert!(TEMPLATE_JSON.contains("\"resources\""));
        assert!(TEMPLATE_JSON.contains("\"tasks\""));
        assert!(TEMPLATE_JSON.contains("\"phases\""));
        assert!(TEMPLATE_JSON.contains("SCHED_OTHER"));
        assert!(TEMPLATE_JSON.contains("calibration"));
    }

    #[test]
    fn colorize_json_preserves_content() {
        // Stripping ANSI codes should yield original content
        let input = r#"{"key": "value", "num": 42, "flag": true}"#;
        let colored = colorize_json(input);
        let stripped = strip_ansi(&colored);
        assert_eq!(stripped, input);
    }

    #[test]
    fn colorize_json_handles_comments() {
        let input = r#"{"key": 1 // comment
}"#;
        let colored = colorize_json(input);
        let stripped = strip_ansi(&colored);
        assert_eq!(stripped, input);
    }

    #[test]
    fn colorize_json_handles_block_comments() {
        let input = r#"{"key": /* comment */ 1}"#;
        let colored = colorize_json(input);
        let stripped = strip_ansi(&colored);
        assert_eq!(stripped, input);
    }

    /// Strip ANSI escape sequences from a string.
    fn strip_ansi(s: &str) -> String {
        let mut result = String::new();
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                // Skip until 'm' (end of ANSI sequence)
                while let Some(c2) = chars.next() {
                    if c2 == 'm' {
                        break;
                    }
                }
            } else {
                result.push(c);
            }
        }
        result
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
