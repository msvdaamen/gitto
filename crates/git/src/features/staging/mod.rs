//! Staging and unstaging files and lines, and discarding changes.
//!
//! Ported from `packages/git/src/features/staging/`.

pub(crate) mod commands;
pub(crate) mod discard;
pub mod lines;
pub mod patch_version;
pub mod schema;
