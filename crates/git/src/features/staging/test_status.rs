//! Each changed file, with whether its change is staged and unstaged, for the tests: the tests'
//! `statusFiles`, `parseStatus(await repo.read(STATUS_ARGS)).files`.

use crate::core::repo::Repo;
use crate::features::status::parse::{STATUS_ARGS, parse_status};
use crate::features::status::schema::StatusConflict;
pub use crate::features::status::schema::StatusFile;
use crate::schema::FileStatus;

impl StatusFile {
    pub fn new(
        path: &str,
        orig_path: Option<&str>,
        staged: Option<FileStatus>,
        unstaged: Option<FileStatus>,
    ) -> Self {
        StatusFile {
            path: path.to_owned(),
            orig_path: orig_path.map(str::to_owned),
            staged,
            unstaged,
            conflict: None,
        }
    }

    pub fn conflicted(path: &str, xy: &str, ours: &str, theirs: &str) -> Self {
        StatusFile {
            path: path.to_owned(),
            orig_path: None,
            staged: Some(FileStatus::Conflicted),
            unstaged: Some(FileStatus::Conflicted),
            conflict: Some(StatusConflict {
                xy: xy.to_owned(),
                ours: ours.to_owned(),
                theirs: theirs.to_owned(),
            }),
        }
    }
}

/// Each changed file, with whether its change is staged and unstaged.
pub async fn status_files(repo: &Repo) -> Vec<StatusFile> {
    parse_status(&repo.read(STATUS_ARGS).await.unwrap()).files
}
