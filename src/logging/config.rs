//! Log configuration resolved from CLI flags and environment variables.
//!
//! Priority (highest first):
//! - `RUST_LOG` — per-target level filter (standard Rust ecosystem)
//! - `--log-level` / `TREBLE_LOG_LEVEL` — default level when `RUST_LOG` is unset
//! - `--log-output` / `TREBLE_LOG_OUTPUT` — destination override
//! - built-in default file under the platform data directory

use std::path::PathBuf;

use clap::Parser;

/// CLI flags and environment overrides for logging.
///
/// Logging is initialized before the terminal enters raw mode so output never
/// corrupts the TUI. All log records go to the configured destination (file
/// or pipe), never to the alternate screen.
#[derive(Debug, Parser, Default)]
#[command(
    name = "treble-tui",
    about = "A vim-like TUI live-coding environment for Treble",
    disable_version_flag = true
)]
pub struct LogCli {
    /// Default log level when `RUST_LOG` is not set (e.g. `trace`, `debug`, `info`).
    #[arg(long, env = "TREBLE_LOG_LEVEL", value_name = "LEVEL")]
    pub log_level: Option<String>,

    /// Log destination: file path, `stdout`, `stderr`, or `-` (stderr pipe).
    #[arg(long, env = "TREBLE_LOG_OUTPUT", value_name = "PATH|stdout|stderr|-")]
    pub log_output: Option<String>,
}

/// Resolved log destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogOutput {
    /// Append to a file (parent directories are created on init).
    File(PathBuf),
    /// Standard output (for shell pipes).
    Stdout,
    /// Standard error.
    Stderr,
}

/// Fully resolved logging configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogConfig {
    /// Optional default level used when `RUST_LOG` is absent.
    pub default_level: Option<String>,
    pub output: LogOutput,
}

impl LogConfig {
    /// Resolve configuration from parsed CLI arguments.
    pub fn from_cli(cli: &LogCli) -> Self {
        Self {
            default_level: cli.log_level.clone(),
            output: resolve_output(cli.log_output.as_deref()),
        }
    }

    /// Default file path when no output override is provided.
    pub fn default_file_path() -> PathBuf {
        directories::ProjectDirs::from("xyz", "minigrim0", "treble-tui")
            .map(|dirs| dirs.data_local_dir().join("logs").join("treble-tui.log"))
            .unwrap_or_else(|| PathBuf::from("treble-tui.log"))
    }
}

fn resolve_output(raw: Option<&str>) -> LogOutput {
    match raw.map(str::trim).filter(|s| !s.is_empty()) {
        None => LogOutput::File(LogConfig::default_file_path()),
        Some("-") | Some("stderr") => LogOutput::Stderr,
        Some("stdout") => LogOutput::Stdout,
        Some(path) => LogOutput::File(PathBuf::from(path)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_output_is_file() {
        let config = LogConfig::from_cli(&LogCli::default());
        assert!(matches!(config.output, LogOutput::File(_)));
    }

    #[test]
    fn stdout_and_stderr_aliases() {
        assert_eq!(
            resolve_output(Some("stdout")),
            LogOutput::Stdout
        );
        assert_eq!(
            resolve_output(Some("stderr")),
            LogOutput::Stderr
        );
        assert_eq!(resolve_output(Some("-")), LogOutput::Stderr);
    }

    #[test]
    fn custom_file_path() {
        assert_eq!(
            resolve_output(Some("/tmp/treble.log")),
            LogOutput::File(PathBuf::from("/tmp/treble.log"))
        );
    }
}
