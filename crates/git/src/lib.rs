//! Gitto's git backend: runs git for the repositories added to Gitto, and reads what it says.
//! Ported from `packages/git` (TypeScript); the UI calls it directly, where the Electron app went
//! through oRPC.
//!
//! - [`api`]: what the UI calls it through, [`GitApi`], and the errors it gets, [`ApiError`].
//! - [`core`]: running git, the repositories it runs in, and the errors.
//! - [`features`]: what Gitto does with git, one module per feature.
//! - [`input`] and [`schema`]: the input and types the features share.

pub mod api;
pub mod core;
pub mod features;
pub mod input;
pub mod schema;

#[cfg(test)]
pub(crate) mod test_support;

pub use crate::api::{ApiError, GitApi, GitRepo};
pub use crate::core::errors::{
    ApiCode, Error, GitError, GitErrorKind, ListAction, RepositoryListError, Result,
};
pub use crate::core::repo::{GitCommand, GitRepos, Repo, RepositoryLookup};
pub use crate::core::runner::RunOptions;
pub use crate::core::version::GitVersion;
pub use crate::schema::{FileStatus, GitInstall};
