//! What the home page shows of a repository. Ported from `overview/schema.ts`.

use serde::{Deserialize, Serialize};

/// What moved HEAD, going by its reflog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActivityKind {
    Commit,
    Amend,
    Merge,
    Pull,
    Rebase,
    Checkout,
    Reset,
    CherryPick,
    Revert,
    Clone,
    Other,
}

impl ActivityKind {
    /// As TS spells it, e.g. `cherry-pick`.
    pub fn as_str(self) -> &'static str {
        match self {
            ActivityKind::Commit => "commit",
            ActivityKind::Amend => "amend",
            ActivityKind::Merge => "merge",
            ActivityKind::Pull => "pull",
            ActivityKind::Rebase => "rebase",
            ActivityKind::Checkout => "checkout",
            ActivityKind::Reset => "reset",
            ActivityKind::CherryPick => "cherry-pick",
            ActivityKind::Revert => "revert",
            ActivityKind::Clone => "clone",
            ActivityKind::Other => "other",
        }
    }
}

/// Something done in the repository that moved HEAD: an entry in its reflog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    /// When, in milliseconds since the epoch.
    pub at: i64,
    pub kind: ActivityKind,
    /// What it was done with, as git names it: the branch or commit switched or reset to, what was
    /// merged or pulled, the branch rebased. `None` if git doesn't name one.
    pub target: Option<String>,
    /// The subject of the commit HEAD was at after it, e.g. the one just made.
    pub subject: String,
    /// Git's own words for it, e.g. `checkout: moving from main to feature`.
    pub message: String,
}

/// The remote a repository is shown as hosted on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostedRemote {
    pub name: String,
    pub url: String,
}

/// What the home page shows of a repository, beyond its status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    /// The remote it's shown as hosted on: `origin`, or else the first one; `None` without any.
    pub remote: Option<HostedRemote>,
    /// When a remote was last fetched from, in milliseconds since the epoch; `None` if never.
    pub fetched_at: Option<i64>,
    /// When HEAD's commit was made, in milliseconds since the epoch; `None` before the first one.
    pub committed_at: Option<i64>,
    /// What was last done in the repository, most recent first.
    pub activity: Vec<Activity>,
}
