//! What every feature builds on: running git ([`runner`]), the repositories it runs in
//! ([`repo`]), and the errors it all returns ([`errors`]). Ported from `packages/git/src/core/`.
//! The oRPC middleware isn't ported: the UI calls the features directly.
//!
//! How git is run (`runner`'s functions, the queues and caches) is the crate's own: the features
//! run git through a [`repo::Repo`].

mod caches;
pub mod errors;
pub mod git_dirs;
mod queue;
pub mod records;
pub mod repo;
pub mod runner;
pub mod shell_path;
pub mod trace;
pub mod version;
