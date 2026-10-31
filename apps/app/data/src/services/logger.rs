//! The log: what the app and GPUI say through the `log` crate, to stderr, as the Electron app's
//! main process wrote to its console, and to `gitto.log` in the `userData` folder (see
//! `AppPaths::log_file`), for an app started where there's no console to see. Errors and warnings
//! always; with `GITTO_TRACE=1`, the switch the git crate's tracing goes by too, everything
//! Gitto's own crates say (GPUI's and the rest's stay at warnings, or they'd drown it out). Each
//! line starts with the local time it was written at.

use std::fs::File;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use log::{Level, LevelFilter, Log, Metadata, Record};

use super::AppPaths;

/// How big `gitto.log` gets before it's moved to `gitto.old.log` and started again: the log keeps
/// up to twice this.
const LOG_FILE_LIMIT: u64 = 5 * 1024 * 1024;

struct Logger {
    /// Whether to log everything Gitto's crates say (`GITTO_TRACE`).
    trace: bool,
    file: Option<Mutex<LogFile>>,
}

impl Logger {
    /// The most this logs of what `target` says.
    fn level(&self, target: &str) -> LevelFilter {
        if self.trace && is_gittos(target) {
            LevelFilter::Trace
        } else {
            LevelFilter::Warn
        }
    }

    fn file(&self) -> Option<MutexGuard<'_, LogFile>> {
        let file = self.file.as_ref()?;
        Some(file.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

/// Whether `target` (a module path, or a target of its own like the git crate's `gitto_git`) is
/// one of Gitto's crates': `gitto_*`, or the app's, `app`.
fn is_gittos(target: &str) -> bool {
    let krate = target.split("::").next().unwrap_or(target);
    krate.starts_with("gitto") || krate == "app"
}

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.level(metadata.target())
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = line(
            record,
            &chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
        );
        let _ = std::io::stderr().lock().write_all(line.as_bytes());
        if let Some(mut file) = self.file() {
            file.write(&line);
        }
    }

    fn flush(&self) {
        if let Some(mut file) = self.file() {
            let _ = file.file.flush();
        }
    }
}

/// `record` as a line of the log, written at `time`.
fn line(record: &Record, time: &impl std::fmt::Display) -> String {
    let level = match record.level() {
        Level::Error => "error",
        Level::Warn => "warn",
        Level::Info => "info",
        Level::Debug => "debug",
        Level::Trace => "trace",
    };
    format!("{time} [{level}] {}: {}\n", record.target(), record.args())
}

/// The log's file, kept to [`LOG_FILE_LIMIT`]: once it's that big, it's moved to the `.old.log`
/// beside it (in place of the one there), and started again.
struct LogFile {
    path: PathBuf,
    file: File,
    /// How big the file is.
    size: u64,
    limit: u64,
}

impl LogFile {
    /// Appends to `path`, creating it and its folder if need be.
    fn open(path: PathBuf, limit: u64) -> io::Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let file = File::options().create(true).append(true).open(&path)?;
        let size = file.metadata()?.len();
        let mut log = LogFile {
            path,
            file,
            size,
            limit,
        };
        if log.size >= log.limit {
            log.start_again()?;
        }
        Ok(log)
    }

    fn write(&mut self, line: &str) {
        if self.file.write_all(line.as_bytes()).is_ok() {
            self.size += line.len() as u64;
        }
        if self.size >= self.limit {
            let _ = self.start_again();
        }
    }

    /// Moves the file to the old one's place, and starts a new one.
    fn start_again(&mut self) -> io::Result<()> {
        std::fs::rename(&self.path, old_log_path(&self.path))?;
        self.file = File::options().create(true).append(true).open(&self.path)?;
        self.size = 0;
        Ok(())
    }
}

/// Where `gitto.log` goes once it's full: `gitto.old.log`.
fn old_log_path(path: &Path) -> PathBuf {
    path.with_extension("old.log")
}

/// Sends the log to stderr and `gitto.log` (where the user data folder can be found, and the
/// file opened); does nothing if a logger is set already, as in tests.
pub fn init() {
    let trace = std::env::var_os("GITTO_TRACE").is_some_and(|value| !value.is_empty());
    let file = AppPaths::resolve().and_then(|paths| {
        let path = paths.log_file();
        LogFile::open(path.clone(), LOG_FILE_LIMIT)
            .inspect_err(|error| eprintln!("Couldn't open the log, {}: {error}", path.display()))
            .ok()
    });
    let logger = Logger {
        trace,
        file: file.map(Mutex::new),
    };
    if log::set_boxed_logger(Box::new(logger)).is_ok() {
        log::set_max_level(if trace {
            LevelFilter::Trace
        } else {
            LevelFilter::Warn
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record<'a>(level: Level, target: &'a str, args: std::fmt::Arguments<'a>) -> Record<'a> {
        Record::builder()
            .level(level)
            .target(target)
            .args(args)
            .build()
    }

    #[test]
    fn traces_only_gittos_crates() {
        let logger = Logger {
            trace: true,
            file: None,
        };
        let enabled = |level, target| {
            logger.enabled(&Metadata::builder().level(level).target(target).build())
        };
        assert!(enabled(Level::Debug, "gitto_git"));
        assert!(enabled(Level::Trace, "gitto_app_data::data::watch"));
        assert!(enabled(Level::Debug, "app::views::root"));
        assert!(!enabled(Level::Debug, "gpui::window"));
        assert!(!enabled(Level::Info, "wgpu_core::device"));
        assert!(enabled(Level::Warn, "wgpu_core::device"));
    }

    #[test]
    fn logs_warnings_and_errors_without_tracing() {
        let logger = Logger {
            trace: false,
            file: None,
        };
        let enabled = |level, target| {
            logger.enabled(&Metadata::builder().level(level).target(target).build())
        };
        assert!(enabled(Level::Error, "gpui"));
        assert!(enabled(Level::Warn, "gitto_git"));
        assert!(!enabled(Level::Debug, "gitto_git"));
    }

    #[test]
    fn writes_a_line_with_the_time() {
        let line = line(
            &record(Level::Warn, "gitto_git", format_args!("Couldn't read")),
            &"2026-10-31 12:00:00.000",
        );
        assert_eq!(
            line,
            "2026-10-31 12:00:00.000 [warn] gitto_git: Couldn't read\n"
        );
    }

    #[test]
    fn keeps_the_log_file_to_its_limit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("logs").join("gitto.log");
        let mut log = LogFile::open(path.clone(), 30).unwrap();
        log.write("first line, 20 long\n");
        log.write("second line, 20 lon\n");
        log.write("third\n");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "third\n");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("logs").join("gitto.old.log")).unwrap(),
            "first line, 20 long\nsecond line, 20 lon\n"
        );

        // Opened again, it's added to.
        drop(log);
        let mut log = LogFile::open(path.clone(), 30).unwrap();
        log.write("fourth\n");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "third\nfourth\n");
    }

    #[test]
    fn starts_a_full_log_file_again_when_its_opened() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gitto.log");
        std::fs::write(&path, "x".repeat(40)).unwrap();
        let mut log = LogFile::open(path.clone(), 30).unwrap();
        log.write("new\n");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new\n");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("gitto.old.log")).unwrap(),
            "x".repeat(40)
        );
    }
}
