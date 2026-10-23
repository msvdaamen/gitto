//! Staging and unstaging files and lines, and discarding changes.
//!
//! Ported from `packages/git/src/features/staging/`.

pub mod commands;
pub mod discard;
pub mod lines;
pub mod patch_version;
pub mod schema;

#[cfg(test)]
mod test_status;
