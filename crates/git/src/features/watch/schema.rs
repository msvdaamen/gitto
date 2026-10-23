//! The watch feature's types. Ported from `schema.ts`.

use serde::{Deserialize, Serialize};

/// What changed in the git directory: `Index` when something was staged or unstaged; `Refs` when
/// HEAD, a branch, a tag, the config or an operation in progress (a merge, a rebase) changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GitDirChange {
    Index,
    Refs,
}

impl GitDirChange {
    /// As TS spells it, e.g. `index`.
    pub fn as_str(self) -> &'static str {
        match self {
            GitDirChange::Index => "index",
            GitDirChange::Refs => "refs",
        }
    }
}
