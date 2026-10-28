//! What the home page shows of a repository, beyond its status: where it's hosted, when it was
//! last fetched and committed to, and what was last done in it.
//!
//! Ported from `packages/git/src/features/overview/`.

pub mod commands;
pub(crate) mod parse;
pub mod schema;
