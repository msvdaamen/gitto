//! Watching the git directory and the working tree for changes.
//!
//! Ported from `packages/git/src/features/watch/`.

mod batches;
pub(crate) mod commands;
mod generator;
pub mod git_dirs;
pub mod schema;
mod turns;
mod watcher;
