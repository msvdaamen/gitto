//! Set `GITTO_TRACE=1` to log every git command and watcher event with its timing, e.g. to see what
//! switching to another editor makes Gitto do. Logged through the `log` crate, at debug level with
//! the target `gitto_git`, wherever the app sends its log.

use std::fmt::Display;
use std::sync::LazyLock;

/// The target the crate logs with, which a logger can filter by.
pub(crate) const TARGET: &str = "gitto_git";

static TRACING: LazyLock<bool> =
    LazyLock::new(|| std::env::var_os("GITTO_TRACE").is_some_and(|value| !value.is_empty()));

/// Whether tracing is on: work that's only done for a trace can be skipped otherwise.
pub fn tracing() -> bool {
    *TRACING
}

/// Logs `message` when tracing; pass `format_args!(..)` to format it only then.
pub fn trace(message: impl Display) {
    if tracing() {
        log::debug!(target: TARGET, "{message}");
    }
}
