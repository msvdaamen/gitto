//! What merging or rebasing a branch did, as the UI is told. Ported from `schema.ts`.

use serde::{Deserialize, Serialize};

/// What merging a branch did: nothing, as the branch had every commit already; moved the branch to
/// it; made a merge commit; or stopped at conflicts, left to resolve and commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MergeOutcome {
    UpToDate,
    FastForward,
    Merged,
    Conflicts,
}

/// What rebasing a branch did: nothing, as it was on top of the other already; moved it to the
/// other, having no commits of its own to replay (or none the other didn't have already, which git
/// leaves out); replayed its commits on top of it; or stopped at conflicts, left to resolve and
/// continue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RebaseOutcome {
    UpToDate,
    FastForward,
    Rebased,
    Conflicts,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcomes_are_spelled_as_in_typescript() {
        assert_eq!(
            serde_json::to_string(&MergeOutcome::UpToDate).unwrap(),
            "\"up-to-date\""
        );
        assert_eq!(
            serde_json::to_string(&RebaseOutcome::FastForward).unwrap(),
            "\"fast-forward\""
        );
    }
}
