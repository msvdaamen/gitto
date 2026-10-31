//! The diff feature's types. Ported from `schema.ts`.

use serde::{Deserialize, Serialize};

use crate::schema::FileStatus;

/// A file a diff changed, with its line counts.
///
/// Its paths are text, as git's are on macOS and Windows and nearly always elsewhere: a name that
/// isn't UTF-8, which only Linux's file systems take, isn't supported. Such a file is listed with
/// U+FFFD in place of what isn't, as it was in TS, and its patch can't be asked for by that path;
/// it's listed nonetheless, rather than left out, so that it's known to have changed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangedFile {
    pub path: String,
    pub status: FileStatus,
    pub orig_path: Option<String>,
    /// `None` for binary files, and for files whose lines weren't counted.
    pub additions: Option<u64>,
    pub deletions: Option<u64>,
}
