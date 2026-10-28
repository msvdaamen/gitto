//! Types shared by the features. Ported from `packages/git/src/schema.ts`; each feature's own
//! types are in its `schema` module, as in TS.

use std::fmt;
use std::ops::Deref;

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

/// A version of something Gitto read, which a change to it is sent back with, so it's only made
/// to what was read: a patch's (`patch_version`), a file's in the working tree (`version_of`), or
/// the status's (`status_snapshot`). What it holds means nothing outside gitto-git; one can be
/// made from a string for tests.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Version(String);

impl Version {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for Version {
    fn from(version: String) -> Self {
        Version(version)
    }
}

impl From<&str> for Version {
    fn from(version: &str) -> Self {
        Version(version.to_owned())
    }
}

impl PartialEq<str> for Version {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for Version {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl Deref for Version {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
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
