//! Gitto's list of repositories: the git repositories added to it, in the order of their tabs,
//! kept in its database (see `gitto-db`). Ported from `packages/repository`; the UI calls
//! [`RepositoryService`] directly, where the Electron app went through oRPC (`contract.ts` and
//! `handler.ts` aren't ported, and TS's `server/` folder is this crate's root).
//!
//! - [`RepositoryService`]: adds, removes, reorders and lists them.
//! - [`RepositoryStore`]: their table in the database.
//! - [`Error`]: why that failed, in words a user can read.
//!
//! [`RepositoryService`] is gitto-git's `RepositoryLookup`: `gitto_git::GitApi::new(service)`
//! opens the repositories added to Gitto, by their id.

mod errors;
mod lookup;
mod service;
mod store;
mod types;

pub use crate::errors::{Error, ListAction, ListError, Result};
pub use crate::service::{RepositoryService, resolve};
pub use crate::store::RepositoryStore;
pub use crate::types::Repository;
