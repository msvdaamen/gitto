//! Types shared by the features. Ported from `packages/git/src/schema.ts`; each feature's own
//! types are in its `schema` module, as in TS.

use std::fmt;
use std::ops::Deref;
use std::str::FromStr;

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

/// What a path is in a tree or the index, by the mode git keeps for it, e.g. `100644`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileMode {
    /// A file: `100644`.
    Regular,
    /// An executable file: `100755`.
    Executable,
    /// A symbolic link: `120000`.
    Symlink,
    /// A submodule, by its commit: `160000`.
    Submodule,
    /// Nothing there, as `git status` reports a side that doesn't have the path: `000000`.
    Absent,
}

impl FileMode {
    /// As git writes it, e.g. `100644`.
    pub fn as_str(self) -> &'static str {
        match self {
            FileMode::Regular => "100644",
            FileMode::Executable => "100755",
            FileMode::Symlink => "120000",
            FileMode::Submodule => "160000",
            FileMode::Absent => "000000",
        }
    }

    /// Whether it's a file's, executable or not, rather than a link's or a submodule's.
    pub fn is_file(self) -> bool {
        matches!(self, FileMode::Regular | FileMode::Executable)
    }
}

impl fmt::Display for FileMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A mode git doesn't write, e.g. a tree's (`040000`), which the index never has.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0} isn't a file's mode")]
pub struct UnknownFileMode(pub String);

impl FromStr for FileMode {
    type Err = UnknownFileMode;

    fn from_str(mode: &str) -> Result<Self, Self::Err> {
        Ok(match mode {
            "100644" => FileMode::Regular,
            "100755" => FileMode::Executable,
            "120000" => FileMode::Symlink,
            "160000" => FileMode::Submodule,
            "000000" => FileMode::Absent,
            _ => return Err(UnknownFileMode(mode.to_owned())),
        })
    }
}

impl Serialize for FileMode {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for FileMode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
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

/// A version of what Gitto read, which a change to it is sent back with, so it's only made to what
/// was read: a file's in the working tree (`version_of`), or the status's (`Uncommitted::version`).
/// A patch's is a `PatchVersion`. What it holds means nothing outside gitto-git; one can be made
/// from a string for tests.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ContentVersion(String);

impl ContentVersion {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for ContentVersion {
    fn from(version: String) -> Self {
        ContentVersion(version)
    }
}

impl From<&str> for ContentVersion {
    fn from(version: &str) -> Self {
        ContentVersion(version.to_owned())
    }
}

impl PartialEq<str> for ContentVersion {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for ContentVersion {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl Deref for ContentVersion {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ContentVersion {
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

    #[test]
    fn file_modes_read_and_write_as_git_does() {
        for mode in ["100644", "100755", "120000", "160000", "000000"] {
            let parsed: FileMode = mode.parse().unwrap();
            assert_eq!(parsed.to_string(), mode);
            assert_eq!(
                serde_json::to_string(&parsed).unwrap(),
                format!("\"{mode}\"")
            );
        }
        assert!("040000".parse::<FileMode>().is_err());
        assert!(FileMode::Executable.is_file());
        assert!(!FileMode::Submodule.is_file());
    }
}
