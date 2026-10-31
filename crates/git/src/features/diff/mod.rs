//! Diffs of the working tree, the index and commits, and editing files in the working tree.
//!
//! Ported from `packages/git/src/features/diff/`.

pub(crate) mod commands;
pub mod hunk_header;
pub mod limits;
pub(crate) mod parse;
pub mod schema;
pub mod working_tree;
