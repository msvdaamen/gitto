//! What every feature builds on: running git ([`runner`]), the repositories it runs in
//! ([`repo`]), and the errors it all returns ([`errors`]). Ported from `packages/git/src/core/`.
//! The oRPC middleware is `crate::api::GitApi`, which the UI calls the features through.

pub mod errors;
pub mod git_dirs;
pub mod repo;
pub mod runner;
pub mod trace;
pub mod version;
