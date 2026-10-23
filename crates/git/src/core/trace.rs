//! Set `GITTO_TRACE=1` to log every git command and watcher event with its timing, e.g. to see what
//! switching to another editor makes Gitto do.

use std::fmt::Display;
use std::sync::LazyLock;

static TRACING: LazyLock<bool> =
    LazyLock::new(|| std::env::var_os("GITTO_TRACE").is_some_and(|value| !value.is_empty()));

/// Whether tracing is on: work that's only done for a trace can be skipped otherwise.
pub fn tracing() -> bool {
    *TRACING
}

/// Logs `message` when tracing; pass `format_args!(..)` to format it only then.
pub fn trace(message: impl Display) {
    if tracing() {
        println!("[git] {message}");
    }
}
