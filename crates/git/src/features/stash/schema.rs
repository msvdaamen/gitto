//! The stash feature's types. Ported from `schema.ts`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stash {
    /// The stash commit's full SHA.
    pub sha: String,
    /// The commit it was made on, which its changes are compared to; the first of the stash
    /// commit's parents (the others hold what was staged, and the untracked files).
    pub base: String,
    /// What git named it, e.g. `WIP on main: 1a2b3c4 Fix the header`.
    pub message: String,
    /// When it was made, in milliseconds since the epoch.
    pub created_at: i64,
}
