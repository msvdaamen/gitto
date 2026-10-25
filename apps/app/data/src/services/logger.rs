//! The log: what the app and GPUI say through the `log` crate, to stderr, as the Electron app's
//! main process wrote to its console. Errors and warnings always; everything with `GITTO_TRACE=1`,
//! the switch the git crate's tracing goes by too.

use std::io::Write as _;

use log::{Level, LevelFilter, Log, Metadata, Record};

struct StderrLogger {
    level: LevelFilter,
}

impl Log for StderrLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let level = match record.level() {
            Level::Error => "error",
            Level::Warn => "warn",
            Level::Info => "info",
            Level::Debug => "debug",
            Level::Trace => "trace",
        };
        let _ = writeln!(
            std::io::stderr().lock(),
            "[{level}] {}: {}",
            record.target(),
            record.args()
        );
    }

    fn flush(&self) {}
}

/// Sends the log to stderr; does nothing if a logger is set already, as in tests.
pub fn init() {
    let level = if std::env::var_os("GITTO_TRACE").is_some_and(|value| !value.is_empty()) {
        LevelFilter::Debug
    } else {
        LevelFilter::Warn
    };
    if log::set_boxed_logger(Box::new(StderrLogger { level })).is_ok() {
        log::set_max_level(level);
    }
}
