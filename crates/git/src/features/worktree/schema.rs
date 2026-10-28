//! The worktrees, as the UI gets them. Ported from `schema.ts`.

use serde::{Deserialize, Serialize};

/// A worktree of the repository: the main one, or a linked one checked out in a folder of its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Worktree {
    /// The folder's absolute path, as git lists it.
    pub path: String,
    /// The folder's name, e.g. `gitto-feature`, which the UI calls the worktree by.
    pub name: String,
    /// The commit HEAD points at; `None` in a bare one, which has none.
    pub head: Option<String>,
    /// The full name of the branch checked out there, e.g. `refs/heads/main`; `None` when detached.
    pub branch: Option<String>,
    /// Whether it's the main worktree, whose git directory the others share: it can't be removed.
    pub main: bool,
    /// Whether it's bare: the main one of a bare repository, with no files to open.
    pub bare: bool,
    /// Whether it's the one the repository was opened as, so the one on show.
    pub current: bool,
    /// Its lock, which keeps it from being pruned or removed; `None` if it isn't locked.
    pub locked: Option<Lock>,
    /// Why it can be pruned, e.g. its folder is gone; `None` if it can't.
    pub prunable: Option<String>,
}

/// A worktree's lock (see `Worktree::locked`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lock {
    /// Why it's locked; `None` if it was locked without saying why.
    pub reason: Option<String>,
}
