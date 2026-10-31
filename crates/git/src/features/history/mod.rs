//! The history: the log over every branch, remote branch and tag, and single commits.
//!
//! Ported from `packages/git/src/features/history/`.

pub mod commands;
pub(crate) mod parse;
pub mod schema;
