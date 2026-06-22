use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const LOG_RETENTION_DAYS: u64 = 7;
const LOG_FILE_PREFIX: &str = "terminal-studio.log";
const LOG_SUBDIR: &str = "logs";

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogLevel {
    Off,
    Error,
    #[default]
    Warn,
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    pub const ALL: &[Self] = &[
        Self::Off,
        Self::Error,
        Self::Warn,
        Self::Info,
        Self::Debug,
        Self::Trace,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Error => "Error",
            Self::Warn => "Warn",
            Self::Info => "Info",
            Self::Debug => "Debug",
            Self::Trace => "Trace",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Off => "No logging",
            Self::Error => "Errors only",
            Self::Warn => "Warnings and errors",
            Self::Info => "General operational info",
            Self::Debug => "Detailed diagnostic info",
            Self::Trace => "Everything (verbose)",
        }
    }

    fn to_filter_directive(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Trace => "trace",
        }
    }
}

/// Returns the log directory path: `{data_dir}/logs/`.
pub fn log_dir() -> Option<PathBuf> {
    crate::util::data_dir().map(|d| d.join(LOG_SUBDIR))
}

/// Guard that must remain alive for the non-blocking file writer to keep flushing.
/// Dropping it flushes remaining entries and shuts down the writer thread.
pub struct LogGuard {
    _file_guard: tracing_appender::non_blocking::WorkerGuard,
}

/// Read just the log_level from settings.json without loading the full AppSettings.
pub fn load_configured_level() -> LogLevel {
    #[derive(Deserialize)]
    struct Partial {
        #[serde(default)]
        log_level: LogLevel,
    }
    crate::util::data_file("settings.json")
        .and_then(|path| crate::util::safe_json_load::<Partial>(&path))
        .map(|p| p.log_level)
        .unwrap_or_default()
}

/// Initialize the logging backend. Must be called once, early in `main()`.
///
/// Returns a guard that must be held alive for the app's lifetime.
/// Returns `None` if the data directory is unavailable or the subscriber
/// could not be installed.
pub fn init(level: LogLevel) -> Option<LogGuard> {
    if level == LogLevel::Off {
        return None;
    }

    let dir = log_dir()?;
    let _ = std::fs::create_dir_all(&dir);

    cleanup_old_logs(&dir);

    let file_appender = tracing_appender::rolling::daily(&dir, LOG_FILE_PREFIX);
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(level.to_filter_directive()));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_target(true)
        .with_thread_names(true)
        .with_file(true)
        .with_line_number(true)
        .try_init()
        .ok()?;

    log::info!(
        "Terminal Studio v{} started (log_level={}, os={}, arch={})",
        env!("CARGO_PKG_VERSION"),
        level.name(),
        std::env::consts::OS,
        std::env::consts::ARCH,
    );

    Some(LogGuard { _file_guard: guard })
}

/// Remove log files older than `LOG_RETENTION_DAYS`.
fn cleanup_old_logs(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let cutoff = std::time::SystemTime::now()
        .checked_sub(std::time::Duration::from_secs(
            LOG_RETENTION_DAYS * 24 * 3600,
        ))
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);

    for entry in entries.flatten() {
        let path = entry.path();
        let is_log_file = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|name| name.starts_with(LOG_FILE_PREFIX));
        if !is_log_file {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        let Ok(modified) = meta.modified() else {
            continue;
        };
        if modified < cutoff {
            let _ = std::fs::remove_file(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_level_default_is_warn() {
        assert_eq!(LogLevel::default(), LogLevel::Warn);
    }

    #[test]
    fn log_level_all_has_six_variants() {
        assert_eq!(LogLevel::ALL.len(), 6);
        assert_eq!(LogLevel::ALL[0], LogLevel::Off);
        assert_eq!(LogLevel::ALL[5], LogLevel::Trace);
    }

    #[test]
    fn log_level_names() {
        let expected = ["Off", "Error", "Warn", "Info", "Debug", "Trace"];
        for (level, name) in LogLevel::ALL.iter().zip(expected.iter()) {
            assert_eq!(level.name(), *name);
        }
    }

    #[test]
    fn log_level_descriptions_are_non_empty() {
        for level in LogLevel::ALL {
            assert!(!level.description().is_empty());
        }
    }

    #[test]
    fn log_level_filter_directives() {
        let expected = ["off", "error", "warn", "info", "debug", "trace"];
        for (level, directive) in LogLevel::ALL.iter().zip(expected.iter()) {
            assert_eq!(level.to_filter_directive(), *directive);
        }
    }

    #[test]
    fn log_level_serde_roundtrip() {
        for &level in LogLevel::ALL {
            let json = serde_json::to_string(&level).unwrap();
            let restored: LogLevel = serde_json::from_str(&json).unwrap();
            assert_eq!(restored, level);
        }
    }

    #[test]
    fn log_dir_returns_logs_subdir() {
        if let Some(dir) = log_dir() {
            assert!(
                dir.ends_with("logs"),
                "log dir should end with 'logs': {:?}",
                dir
            );
        }
    }

    #[test]
    fn cleanup_handles_missing_dir() {
        cleanup_old_logs(Path::new("/nonexistent/path/that/does/not/exist"));
    }

    #[test]
    fn cleanup_preserves_recent_files() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();

        let recent = dir.join("terminal-studio.log.2026-06-22");
        std::fs::write(&recent, "recent data").unwrap();

        let unrelated = dir.join("other-file.txt");
        std::fs::write(&unrelated, "unrelated").unwrap();

        cleanup_old_logs(dir);

        assert!(recent.exists(), "recent log file should be preserved");
        assert!(unrelated.exists(), "non-log file should be untouched");
    }

    #[test]
    fn load_configured_level_defaults_on_missing_file() {
        assert_eq!(load_configured_level(), LogLevel::default());
    }
}
