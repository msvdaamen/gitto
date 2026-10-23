//! The status feature's types. Ported from `schema.ts`.

use serde::{Deserialize, Serialize};

use crate::features::diff::schema::ChangedFile;
use crate::schema::FileStatus;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusFile {
    pub path: String,
    /// Previous path, for renames and copies.
    pub orig_path: Option<String>,
    /// Change in the index relative to HEAD; `None` when nothing is staged.
    pub staged: Option<FileStatus>,
    /// Change in the working tree relative to the index; `None` when there's nothing unstaged.
    pub unstaged: Option<FileStatus>,
    /// How a conflicted file conflicts (see [`StatusConflict`]); only on conflicted files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conflict: Option<StatusConflict>,
}

/// How a conflicted file conflicts, as `git status` says: `UU` both modified, `AA` both added,
/// `UD` and `DU` deleted by them and by us, `AU` and `UA` added by us and by them only, `DD`
/// deleted by both; with the modes ours and theirs have (`000000` where it's not there).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusConflict {
    pub xy: String,
    pub ours: String,
    pub theirs: String,
}

/// What HEAD points at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Head {
    /// A branch, at commit `sha`.
    Branch { name: String, sha: String },
    /// A branch without commits yet, e.g. in a new repository.
    Unborn { name: String },
    /// A commit, with no branch checked out.
    Detached { sha: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub head: Head,
    pub upstream: Option<String>,
    pub ahead: u64,
    pub behind: u64,
    pub files: Vec<StatusFile>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkingTreeFiles {
    /// Changes in the index, compared to HEAD: what the next commit will contain.
    pub staged: Vec<ChangedFile>,
    /// Changes in the working tree, compared to the index, followed by the untracked files.
    pub unstaged: Vec<ChangedFile>,
    /// Whether the files' lines weren't counted, as there are too many to diff on every change:
    /// none has line counts then, binary or not.
    pub uncounted: bool,
    /// The conflicted text files, changed or added on both sides, that have no conflict markers
    /// left: resolved in the working tree, but not marked resolved (staged) yet.
    pub marker_free: Vec<String>,
}

/// How many files changed. A conflict isn't counted as staged or unstaged, though git reports it
/// on both sides; `files` counts each changed file once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusCounts {
    pub files: u64,
    pub staged: u64,
    pub unstaged: u64,
    pub conflicted: u64,
}

/// Where HEAD is, and how much changed: what the UI shows outside the list of changed files.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusSummary {
    pub head: Head,
    pub upstream: Option<String>,
    pub ahead: u64,
    pub behind: u64,
    pub counts: StatusCounts,
}

/// Everything uncommitted: the summary and the changed files, with their line counts if any.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Uncommitted {
    pub head: Head,
    pub upstream: Option<String>,
    pub ahead: u64,
    pub behind: u64,
    pub counts: StatusCounts,
    pub changes: WorkingTreeFiles,
    /// Changes whenever anything above does, so a caller can ask to skip a status it already has.
    pub version: String,
}

impl Uncommitted {
    /// The summary alone, without the changed files.
    pub fn summary(&self) -> StatusSummary {
        StatusSummary {
            head: self.head.clone(),
            upstream: self.upstream.clone(),
            ahead: self.ahead,
            behind: self.behind,
            counts: self.counts,
        }
    }
}

/// What `status.get` answers (the contract's output): `{ unchanged: true }` when the caller
/// already has the status of the `version` it gave, the whole status otherwise.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StatusReply {
    Unchanged { unchanged: Unchanged },
    Status(Box<Uncommitted>),
}

/// `true`, always: `{ unchanged: true }`'s literal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Unchanged;

impl Serialize for Unchanged {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bool(true)
    }
}

impl<'de> Deserialize<'de> for Unchanged {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if bool::deserialize(deserializer)? {
            Ok(Unchanged)
        } else {
            Err(serde::de::Error::custom("expected `true`"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heads_and_replies_are_spelled_as_in_ts() {
        let head = Head::Branch {
            name: "main".into(),
            sha: "1111".into(),
        };
        assert_eq!(
            serde_json::to_string(&head).unwrap(),
            r#"{"kind":"branch","name":"main","sha":"1111"}"#
        );
        let reply = StatusReply::Unchanged {
            unchanged: Unchanged,
        };
        assert_eq!(
            serde_json::to_string(&reply).unwrap(),
            r#"{"unchanged":true}"#
        );
        let file = StatusFile {
            path: "a".into(),
            orig_path: None,
            staged: None,
            unstaged: Some(FileStatus::Untracked),
            conflict: None,
        };
        assert_eq!(
            serde_json::to_string(&file).unwrap(),
            r#"{"path":"a","origPath":null,"staged":null,"unstaged":"untracked"}"#
        );
    }
}
