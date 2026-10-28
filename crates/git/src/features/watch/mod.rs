//! Watching the git directory and the working tree for changes.
//!
//! Ported from `packages/git/src/features/watch/`.

mod batches;
pub mod commands;
pub mod git_dirs;
pub mod schema;
mod stream;
mod watcher;
