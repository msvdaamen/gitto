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

/// What git has at a path, by its mode in a tree or the index, as git writes it (`100644`, say).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileMode {
    /// `100644`.
    File,
    /// `100755`.
    Executable,
    /// `120000`: a symbolic link, whose contents are the path it points to.
    Link,
    /// `160000`: a submodule, at a commit.
    Submodule,
}

impl FileMode {
    /// The mode `mode` is, as git writes it; `None` for another, e.g. a folder's or none
    /// (`000000`).
    pub fn parse(mode: &str) -> Option<Self> {
        Some(match mode {
            "100644" => FileMode::File,
            "100755" => FileMode::Executable,
            "120000" => FileMode::Link,
            "160000" => FileMode::Submodule,
            _ => return None,
        })
    }

    /// As git writes it, e.g. `120000`.
    pub fn as_str(self) -> &'static str {
        match self {
            FileMode::File => "100644",
            FileMode::Executable => "100755",
            FileMode::Link => "120000",
            FileMode::Submodule => "160000",
        }
    }

    /// Whether it's a file's, executable or not, rather than a link's or a submodule's.
    pub fn is_file(self) -> bool {
        matches!(self, FileMode::File | FileMode::Executable)
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

    #[test]
    fn reads_the_file_modes_as_git_writes_them() {
        for mode in [
            FileMode::File,
            FileMode::Executable,
            FileMode::Link,
            FileMode::Submodule,
        ] {
            assert_eq!(FileMode::parse(mode.as_str()), Some(mode));
        }
        assert_eq!(FileMode::parse("040000"), None);
        assert_eq!(FileMode::parse("000000"), None);
        assert!(FileMode::Executable.is_file());
        assert!(!FileMode::Submodule.is_file());
    }
}
