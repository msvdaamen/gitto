//! Diffs of the working tree, the index and commits, and editing files in the working tree.
//!
//! Ported from `packages/git/src/features/diff/`.

pub mod commands;
pub mod hunk_header;
pub mod limits;
pub mod parse;
pub mod schema;
pub mod working_tree;
