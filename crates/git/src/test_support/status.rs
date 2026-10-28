//! The changed files in a repository's status, as the status feature reads them, for the tests of
//! what changes it: the tests' `statusFiles`, `parseStatus(await repo.read(STATUS_ARGS)).files`.

use crate::core::repo::Repo;
use crate::features::status::parse::{STATUS_ARGS, parse_status};
use crate::features::status::schema::StatusConflict;
use crate::features::status::schema::StatusFile;
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
pub async fn status_entries(repo: &Repo) -> Vec<StatusFile> {
    parse_status(&repo.read(STATUS_ARGS).await.unwrap()).files
}

/// A changed file's path, and how it changed in the index and the working tree.
pub type Row = (String, Option<FileStatus>, Option<FileStatus>);

/// Each changed file, with how it changed in the index and the working tree.
pub async fn status_files(repo: &Repo) -> Vec<Row> {
    status_entries(repo)
        .await
        .into_iter()
        .map(|file| (file.path, file.staged, file.unstaged))
        .collect()
}

/// `rows` as `status_files` has them.
pub fn rows(rows: &[(&str, Option<FileStatus>, Option<FileStatus>)]) -> Vec<Row> {
    rows.iter()
        .map(|(path, staged, unstaged)| (path.to_string(), *staged, *unstaged))
        .collect()
}
