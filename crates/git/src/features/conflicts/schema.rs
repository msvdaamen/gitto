//! The conflicts feature's types. Ported from `schema.ts`.

use serde::{Deserialize, Serialize};

use crate::schema::{ContentVersion, FileMode};

/// One side of a conflicted file, as the index has it while it's unmerged: its mode (a file's, a
/// symbolic link's or a submodule's) and its object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictSide {
    pub mode: FileMode,
    pub oid: String,
}

/// The sides of a conflicted file: `None` where the file isn't there, e.g. deleted on one side, or
/// added on both, without a common ancestor's. Ours is HEAD's: the branch merged into, or, in a
/// rebase, the one rebased onto; theirs is what's merged, cherry-picked, or replayed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictSides {
    pub base: Option<ConflictSide>,
    pub ours: Option<ConflictSide>,
    pub theirs: Option<ConflictSide>,
}

/// The file in the working tree as text, with its version (see `read_working_tree_file`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictText {
    pub contents: String,
    pub version: ContentVersion,
}

/// A conflicted file: its sides, and what's in the working tree at its path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Conflict {
    #[serde(flatten)]
    pub sides: ConflictSides,
    pub file: ConflictFile,
}

/// What's in the working tree at a conflicted file's path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ConflictFile {
    /// Text, conflict markers and all, to resolve its conflicts in.
    Text(ConflictText),
    /// A binary file, which git leaves as ours, without markers. `version` is of what's in it.
    Binary { version: ContentVersion },
    /// A file too large to show (see `MAX_BLOB_BYTES`). `stamp` is when it last changed, which
    /// keeping a side checks, rather than what's in it, which would be read again whenever the
    /// working tree changes.
    TooLarge { stamp: String },
    /// A file that isn't UTF-8 text, which can't be shown or edited as text. `version` is of
    /// what's in it.
    NotUtf8 { version: ContentVersion },
    /// A path that leads out of the repository, e.g. in a folder that's a link out of it: not
    /// read.
    Outside,
    /// Nothing, e.g. as it was deleted.
    Missing,
    /// A symbolic link, which isn't followed: it's no part of the conflict, and can lead anywhere.
    /// Its conflict is resolved by keeping a side.
    Link,
}

impl ConflictFile {
    /// Its version, which resolving the conflict by keeping a side checks it still has; `None`
    /// for what can't be read.
    pub fn version(&self) -> Option<FileVersion> {
        match self {
            ConflictFile::Text(text) => Some(FileVersion::Content(text.version.clone())),
            ConflictFile::Binary { version } | ConflictFile::NotUtf8 { version } => {
                Some(FileVersion::Content(version.clone()))
            }
            ConflictFile::TooLarge { stamp } => Some(FileVersion::Stamp(stamp.clone())),
            ConflictFile::Outside | ConflictFile::Missing | ConflictFile::Link => None,
        }
    }

    /// The text, if it's text.
    pub fn text(&self) -> Option<&ConflictText> {
        match self {
            ConflictFile::Text(text) => Some(text),
            _ => None,
        }
    }
}

/// A version of a file in the working tree, to tell whether it changed since.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FileVersion {
    /// Of what's in it (see `read_working_tree_file`).
    Content(ContentVersion),
    /// Of when it last changed, going by what the file system says of it.
    Stamp(String),
}

/// Which side of a conflict to keep whole (TS's `z.enum(["ours", "theirs"])`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Ours,
    Theirs,
}

impl Side {
    /// As git spells it, e.g. in `checkout --ours`.
    pub fn as_str(self) -> &'static str {
        match self {
            Side::Ours => "ours",
            Side::Theirs => "theirs",
        }
    }
}
