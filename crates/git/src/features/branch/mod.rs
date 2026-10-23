//! Branches: creating, switching, deleting, merging and rebasing them.
//!
//! Ported from `packages/git/src/features/branch/`.

pub mod commands;
pub mod delete;
pub mod in_use;
pub mod merge;
pub mod rebase;
pub mod schema;
