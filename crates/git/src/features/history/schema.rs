//! Commits, as the history lists them. Ported from `history/schema.ts`.

use serde::{Deserialize, Serialize};

use crate::features::refs::schema::RefKind;

/// Which kind of ref points at a commit: a detached `HEAD`, or a ref's kind (see [`RefKind`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CommitRefKind {
    Head,
    Local,
    Remote,
    Tag,
}

impl From<RefKind> for CommitRefKind {
    fn from(kind: RefKind) -> Self {
        match kind {
            RefKind::Local => CommitRefKind::Local,
            RefKind::Remote => CommitRefKind::Remote,
            RefKind::Tag => CommitRefKind::Tag,
        }
    }
}

/// A ref pointing at a commit: a branch or tag, e.g. local `main` or tag `v1`, or `HEAD` when it's
/// detached. A checked-out branch is marked `current` instead of getting a `HEAD` of its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitRef {
    pub kind: CommitRefKind,
    /// Short name, e.g. `main`, `origin/main`, `v1`; `HEAD` for a detached head.
    pub name: String,
    /// Full name, e.g. `refs/heads/main`, `refs/remotes/origin/main`; `HEAD` for a detached head.
    pub full_name: String,
    /// Whether it's the checked-out branch. Left out of the JSON (TS's `undefined`) unless it is.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub current: bool,
}

/// A commit, as the history lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Commit {
    pub sha: String,
    pub parents: Vec<String>,
    pub author_name: String,
    pub author_email: String,
    /// Author date in milliseconds since the epoch: when the change was first written.
    pub authored_at: i64,
    /// Commit date in milliseconds since the epoch: when the commit was made, which a rebase or
    /// amend moves to that moment. The history is sorted by it.
    pub committed_at: i64,
    /// Refs pointing at this commit; a checked-out branch is marked `current`.
    pub refs: Vec<CommitRef>,
    pub subject: String,
    pub body: String,
}
