//! The repository type. Ported from `packages/repository/src/types.ts`.

use gitto_git::input::RepositoryId;

/// A git repository added to Gitto: a tab.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Repository {
    /// Made when it was added.
    pub id: RepositoryId,
    /// Its folder's name.
    pub name: String,
    /// Its folder: absolute, as it was added, without following symlinks.
    pub path: String,
}
