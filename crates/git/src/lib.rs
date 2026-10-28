//! Gitto's git backend: runs git for the repositories added to Gitto, and reads what it says.
//! Ported from `packages/git` (TypeScript); the UI calls it directly, where the Electron app went
//! through oRPC.
//!
//! - [`api`]: what the UI calls it through, [`GitApi`], and the errors it gets, [`ApiError`].
//! - `core`, the crate's own: running git, the repositories it runs in ([`Repo`]), and the errors
//!   ([`Error`]), which are re-exported here.
//! - [`features`]: what Gitto does with git, one module per feature. What the UI uses of them
//!   besides their schemas and commands is re-exported here too.
//! - [`input`] and [`schema`]: the input and types the features share.

pub mod api;
pub(crate) mod core;
pub mod features;
pub mod input;
pub mod schema;

#[cfg(test)]
pub(crate) mod test_support;

pub use crate::api::{ApiError, GitApi, GitRepo};
pub use crate::core::errors::{
    ApiCode, BranchDeletionBlocker, DiscardBlocker, DiskChange, Error, GitError, GitErrorKind,
    InUse, Integration, IntegrationBlocker, ListAction, RepositoryListError, Result, StageAction,
    WorktreeRemovalBlocker,
};
pub use crate::core::repo::{GitRepos, Repo, RepositoryLookup};
pub use crate::core::runner::stop_commands_on_exit;
pub use crate::core::shell_path::use_path_from_shell;
pub use crate::core::version::{GitVersion, Version};
pub use crate::features::branch::tracking::{local_name, pick_tracking};
pub use crate::features::conflicts::commands::ShownConflict;
pub use crate::features::diff::working_tree::{SaveExpectation, WorkingTreeFile};
pub use crate::features::history::commands::Page;
pub use crate::features::remote::blockers::{PullBlocker, PushBlocker, pull_blocker};
pub use crate::features::remote::remote_name::remote_of;
pub use crate::features::staging::discard::{DiscardSide, Discarded};
pub use crate::features::staging::patch_version::PatchVersion;
pub use crate::features::user::commands::get_user_name;
pub use crate::schema::{ContentVersion, FileMode, FileStatus, GitInstall};
