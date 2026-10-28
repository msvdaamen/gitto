//! The conflicts feature's types. Ported from `schema.ts`.

use serde::{Deserialize, Serialize};

use crate::schema::FileMode;

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
    pub version: String,
}

/// A conflicted file: its sides, and what's in the working tree at its path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Conflict {
    #[serde(flatten)]
    pub sides: ConflictSides,
    /// The file in the working tree as text, conflict markers and all, with its version (see
    /// `read_working_tree_file`); `None` if it isn't text that can be shown (see `unreadable`).
    pub text: Option<ConflictText>,
    /// The version of the file in the working tree, text or not, which resolving it whole checks
    /// it still has; `None` if there's no file there, or it's a symbolic link, or leads out of the
    /// repository, none of which can be read.
    pub version: Option<String>,
    /// Whether the file in the working tree is binary, which git leaves as ours without markers.
    pub binary: bool,
    /// Why the file in the working tree isn't `text`, e.g. it's too large; `None` if it is.
    pub unreadable: Option<String>,
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
