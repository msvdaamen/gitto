//! What every feature builds on: running git ([`runner`]), the repositories it runs in
//! ([`repo`]), and the errors it all returns ([`errors`]). Ported from `packages/git/src/core/`.
//! The oRPC middleware isn't ported: the UI calls the features directly.

pub mod errors;
pub mod git_dirs;
pub mod repo;
pub mod runner;
pub mod trace;
pub mod version;
