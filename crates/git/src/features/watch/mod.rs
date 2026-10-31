//! Watching the git directory and the working tree for changes.
//!
//! Ported from `packages/git/src/features/watch/`.

mod batches;
pub(crate) mod commands;
pub mod git_dirs;
#[cfg(target_os = "linux")]
mod linux;
pub mod schema;
mod stream;
mod watcher;
