//! Branches: creating, switching, deleting, merging and rebasing them.
//!
//! Ported from `packages/git/src/features/branch/`.

pub(crate) mod commands;
pub(crate) mod delete;
pub mod in_use;
pub(crate) mod merge;
pub(crate) mod rebase;
pub mod schema;
