//! The conflicts feature's types. Ported from `schema.ts`.

use serde::{Deserialize, Serialize};

use crate::schema::{FileMode, Version};

/// One side of a conflicted file, as the index has it while it's unmerged: its mode (`100644`, say,
/// or `120000` for a symbolic link and `160000` for a submodule) and its object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictSide {
    pub mode: String,
    pub oid: String,
}

impl ConflictSide {
    /// What the side has (see [`FileMode`]); `None` for a mode git doesn't give a file.
    pub fn file_mode(&self) -> Option<FileMode> {
        FileMode::parse(&self.mode)
    }
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
    pub version: Version,
}

/// A conflicted file: its sides, and what's in the working tree at its path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Conflict {
    pub base: Option<ConflictSide>,
    pub ours: Option<ConflictSide>,
    pub theirs: Option<ConflictSide>,
    /// The file in the working tree as text, conflict markers and all, with its version (see
    /// `read_working_tree_file`); `None` if it isn't text that can be shown (see `unreadable`).
    pub text: Option<ConflictText>,
    /// The version of the file in the working tree, text or not, which resolving it whole checks
    /// it still has; `None` if there's no file there, or it's a symbolic link, or leads out of the
    /// repository, none of which can be read.
    pub version: Option<Version>,
    /// Whether the file in the working tree is binary, which git leaves as ours without markers.
    pub binary: bool,
    /// Why the file in the working tree isn't `text`, e.g. it's too large; `None` if it is.
    pub unreadable: Option<String>,
}

impl Conflict {
    /// Its sides alone.
    pub fn sides(&self) -> ConflictSides {
        ConflictSides {
            base: self.base.clone(),
            ours: self.ours.clone(),
            theirs: self.theirs.clone(),
        }
    }
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
