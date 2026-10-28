//! Remotes: fetching, pulling and pushing.
//!
//! Ported from `packages/git/src/features/remote/`.

pub mod blockers;
pub(crate) mod commands;
pub mod config;
pub mod progress;
pub(crate) mod push;
pub mod remote_name;
pub mod schema;
