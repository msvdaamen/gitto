//! Types shared by the features. Ported from `packages/git/src/schema.ts`; each feature's own
//! types are in its `schema` module, as in TS.

use serde::{Deserialize, Serialize};

/// How a file changed; shared by the status and diffs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileStatus {
    Modified,
    Added,
    Deleted,
    Renamed,
    Copied,
    Typechange,
    Untracked,
    Conflicted,
}

impl FileStatus {
    /// As TS spells it, e.g. `typechange`.
    pub fn as_str(self) -> &'static str {
        match self {
            FileStatus::Modified => "modified",
            FileStatus::Added => "added",
            FileStatus::Deleted => "deleted",
            FileStatus::Renamed => "renamed",
            FileStatus::Copied => "copied",
            FileStatus::Typechange => "typechange",
            FileStatus::Untracked => "untracked",
            FileStatus::Conflicted => "conflicted",
        }
    }
}

/// The git Gitto runs, and whether Gitto works with it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitInstall {
    /// As git reports it, e.g. `2.39.3`; `None` if git couldn't be run (it isn't installed, say).
    pub version: Option<String>,
    /// The oldest version Gitto works with, e.g. `2.41`.
    pub required: String,
    /// Whether `version` is `required` or newer.
    pub supported: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_statuses_are_spelled_as_in_ts() {
        assert_eq!(
            serde_json::to_string(&FileStatus::Typechange).unwrap(),
            "\"typechange\""
        );
        assert_eq!(FileStatus::Untracked.as_str(), "untracked");
    }
}
