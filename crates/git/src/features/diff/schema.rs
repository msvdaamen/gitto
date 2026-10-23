//! The diff feature's types. Ported from `schema.ts`.

use serde::{Deserialize, Serialize};

use crate::schema::FileStatus;

/// A file a diff changed, with its line counts.
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
