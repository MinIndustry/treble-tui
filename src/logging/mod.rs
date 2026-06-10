//! Non-blocking file/pipe logging for the TUI and the [`treble`] audio engine.
//!
//! # Architecture
//!
//! The TUI and `treble` both emit records through the [`log`] facade. This
//! module installs a [`tracing`] subscriber backed by a [`tracing_appender::non_blocking`]
//! writer so the hot path (audio render thread, terminal event loop) never blocks
//! on disk I/O. A dedicated worker thread flushes formatted lines asynchronously.
//!
//! [`tracing-subscriber`] automatically bridges the [`log`] facade (via its built-in
//! `tracing-log` integration), so `treble`'s existing `log::` calls work without
//! a separate `LogTracer::init()`.
//!
//! # Configuration
//!
//! | Variable / flag | Purpose |
//! |---|---|
//! | `RUST_LOG` | Per-target filter, e.g. `treble=debug,treble_tui=info` |
//! | `TREBLE_LOG_LEVEL` / `--log-level` | Default level when `RUST_LOG` is unset |
//! | `TREBLE_LOG_OUTPUT` / `--log-output` | File path, `stdout`, `stderr`, or `-` |
//!
//! Default destination: `{data_local_dir}/logs/treble-tui.log` (cross-platform via
//! [`directories`]).
//!
//! # Record format
//!
//! Each line includes UTC timestamp, level, thread name/id, source location
//! (file:line), target module, and message.

mod config;

pub use config::{LogCli, LogConfig, LogOutput};

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

use thiserror::Error;
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

/// Keeps the non-blocking writer worker thread alive for the process lifetime.
pub struct LogGuard {
    _worker: WorkerGuard,
}

#[derive(Debug, Error)]
pub enum LogInitError {
    #[error("failed to create log directory {path}: {source}")]
    CreateDir {
        path: String,
        source: io::Error,
    },
    #[error("failed to open log file {path}: {source}")]
    OpenFile {
        path: String,
        source: io::Error,
    },
    #[error("invalid log filter: {0}")]
    Filter(String),
    #[error("failed to initialize tracing subscriber: {0}")]
    SubscriberInit(String),
}

/// Initialize logging from `config`.
///
/// Must be called once, before any [`log`] or [`tracing`] macros and before the
/// terminal enters raw/alternate-screen mode.
pub fn init(config: &LogConfig) -> Result<LogGuard, LogInitError> {
    let filter = build_filter(config)?;
    let (writer, worker) = open_writer(&config.output)?;

    tracing_subscriber::registry()
        .with(filter)
        .with(
            fmt::layer()
                .with_writer(writer)
                .with_ansi(false)
                .with_file(true)
                .with_line_number(true)
                .with_thread_ids(true)
                .with_thread_names(true)
                .with_target(true)
                .with_timer(fmt::time::ChronoUtc::rfc_3339()),
        )
        .try_init()
        .map_err(|e| LogInitError::SubscriberInit(e.to_string()))?;

    log_startup(config);

    Ok(LogGuard { _worker: worker })
}

fn build_filter(config: &LogConfig) -> Result<EnvFilter, LogInitError> {
    if let Ok(filter) = EnvFilter::try_from_env("RUST_LOG") {
        return Ok(filter);
    }

    let default = config
        .default_level
        .as_deref()
        .unwrap_or("treble_tui=info,treble=info,treble_lang=warn");

    EnvFilter::try_new(default).map_err(|e| LogInitError::Filter(e.to_string()))
}

fn open_writer(output: &LogOutput) -> Result<(NonBlocking, WorkerGuard), LogInitError> {
    match output {
        LogOutput::Stdout => {
            let (writer, guard) = tracing_appender::non_blocking(std::io::stdout());
            Ok((writer, guard))
        }
        LogOutput::Stderr => {
            let (writer, guard) = tracing_appender::non_blocking(std::io::stderr());
            Ok((writer, guard))
        }
        LogOutput::File(path) => open_file_writer(path),
    }
}

fn open_file_writer(path: &Path) -> Result<(NonBlocking, WorkerGuard), LogInitError> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|source| LogInitError::CreateDir {
            path: parent.display().to_string(),
            source,
        })?;
    }

    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|source| LogInitError::OpenFile {
            path: path.display().to_string(),
            source,
        })?;

    let (writer, guard) = tracing_appender::non_blocking(file);
    Ok((writer, guard))
}

fn log_startup(config: &LogConfig) {
    let destination = match &config.output {
        LogOutput::File(path) => path.display().to_string(),
        LogOutput::Stdout => "stdout".to_string(),
        LogOutput::Stderr => "stderr".to_string(),
    };

    log::info!(
        target: "treble_tui::logging",
        "logging initialized (non-blocking writer, destination={destination})"
    );
}

/// Write a fatal initialization message when logging is not yet available.
pub fn emit_init_failure(err: &LogInitError) {
    let _ = writeln!(io::stderr(), "treble-tui: logging init failed: {err}");
}
