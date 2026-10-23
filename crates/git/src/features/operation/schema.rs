//! The operation under way in a repository, as the UI is told of it. Ported from
//! `packages/git/src/features/operation/schema.ts`.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A commit, as it's named to the user: its abbreviated SHA and its subject.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitName {
    pub sha: String,
    pub subject: String,
}

/// How far an operation that goes commit by commit has got: at `step` of `total`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Steps {
    pub step: u64,
    pub total: u64,
}

/// An operation git left under way in the repository, for the user to finish or abort: a merge, a
/// rebase, cherry-picks or reverts, or `git am`. Names are as short as they can be said: `feature`,
/// `origin/main`, or an abbreviated SHA when no branch or tag points at the commit.
///
/// TS has cherry-picks and reverts share one shape, `kind` being either; here each is a variant of
/// its own, with the same fields, which serializes the same.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Operation {
    Merge {
        /// What's being merged.
        merging: String,
        /// The branch it's merged into; `None` with HEAD detached.
        into: Option<String>,
    },
    Rebase {
        /// The branch being rebased; `None` if it's a detached HEAD.
        branch: Option<String>,
        onto: String,
        /// The commit being replayed, of how many.
        steps: Option<Steps>,
    },
    #[serde(rename = "cherry-pick", rename_all = "camelCase")]
    CherryPick {
        /// The commit being cherry-picked; `None` between the commits of a series, once the one it
        /// stopped at was committed by hand.
        commit: Option<CommitName>,
        /// How many more are to be after it, in a series of them.
        remaining: u64,
    },
    #[serde(rename_all = "camelCase")]
    Revert {
        /// The commit being reverted; `None` between the commits of a series, once the one it
        /// stopped at was committed by hand.
        commit: Option<CommitName>,
        /// How many more are to be after it, in a series of them.
        remaining: u64,
    },
    Am {
        /// The patch being applied, of how many.
        steps: Option<Steps>,
    },
}

impl Operation {
    /// Which operation this is.
    pub fn kind(&self) -> OperationKind {
        match self {
            Operation::Merge { .. } => OperationKind::Merge,
            Operation::Rebase { .. } => OperationKind::Rebase,
            Operation::CherryPick { .. } => OperationKind::CherryPick,
            Operation::Revert { .. } => OperationKind::Revert,
            Operation::Am { .. } => OperationKind::Am,
        }
    }
}

/// Which operation is under way: the `kind` of an [`Operation`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OperationKind {
    #[serde(rename = "merge")]
    Merge,
    #[serde(rename = "rebase")]
    Rebase,
    #[serde(rename = "cherry-pick")]
    CherryPick,
    #[serde(rename = "revert")]
    Revert,
    #[serde(rename = "am")]
    Am,
}

impl OperationKind {
    /// As TS spells it, which is also the git command that continues or aborts it: `cherry-pick`,
    /// `am`.
    pub fn as_str(self) -> &'static str {
        match self {
            OperationKind::Merge => "merge",
            OperationKind::Rebase => "rebase",
            OperationKind::CherryPick => "cherry-pick",
            OperationKind::Revert => "revert",
            OperationKind::Am => "am",
        }
    }

    /// What the operation is called in a sentence, e.g. "a merge" without its article (TS's
    /// `OPERATION_NAMES`).
    pub fn name(self) -> &'static str {
        match self {
            OperationKind::Merge => "merge",
            OperationKind::Rebase => "rebase",
            OperationKind::CherryPick => "cherry-pick",
            OperationKind::Revert => "revert",
            OperationKind::Am => "git am",
        }
    }
}

impl fmt::Display for OperationKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operations_are_spelled_as_in_ts() {
        let operation = Operation::CherryPick {
            commit: Some(CommitName {
                sha: "abc1234".into(),
                subject: "theirs".into(),
            }),
            remaining: 2,
        };
        assert_eq!(
            serde_json::to_value(&operation).unwrap(),
            serde_json::json!({
                "kind": "cherry-pick",
                "commit": { "sha": "abc1234", "subject": "theirs" },
                "remaining": 2,
            })
        );
        let rebase = Operation::Rebase {
            branch: None,
            onto: "main".into(),
            steps: Some(Steps { step: 1, total: 2 }),
        };
        assert_eq!(
            serde_json::to_value(&rebase).unwrap(),
            serde_json::json!({
                "kind": "rebase",
                "branch": null,
                "onto": "main",
                "steps": { "step": 1, "total": 2 },
            })
        );
        assert_eq!(
            serde_json::from_str::<OperationKind>("\"cherry-pick\"").unwrap(),
            OperationKind::CherryPick
        );
        assert_eq!(OperationKind::Am.name(), "git am");
    }
}
