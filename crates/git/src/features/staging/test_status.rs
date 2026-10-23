//! Each changed file, with whether its change is staged and unstaged, for the tests: the tests'
//! `statusFiles`, `parseStatus(await repo.read(STATUS_ARGS)).files`, read as the status feature's
//! `parseStatus` reads them.
//!
//! The status feature isn't ported yet, so this is the part of its parser the tests use; once it
//! is, `status_files` can read with it instead.

use crate::core::repo::Repo;
use crate::schema::FileStatus;

/// `git status --porcelain=v2 -z --branch`; see "Porcelain Format Version 2" in git-status(1).
const STATUS_ARGS: [&str; 5] = [
    "status",
    "--porcelain=v2",
    "-z",
    "--branch",
    "--untracked-files=all",
];

/// A conflicted file's `xy` code, and its modes on our side and theirs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub xy: String,
    pub ours: String,
    pub theirs: String,
}

/// One changed file, as the status's `files` has it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusFile {
    pub path: String,
    pub orig_path: Option<String>,
    pub staged: Option<FileStatus>,
    pub unstaged: Option<FileStatus>,
    pub conflict: Option<Conflict>,
}

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
            conflict: Some(Conflict {
                xy: xy.to_owned(),
                ours: ours.to_owned(),
                theirs: theirs.to_owned(),
            }),
        }
    }
}

/// Each changed file, with whether its change is staged and unstaged.
pub async fn status_files(repo: &Repo) -> Vec<StatusFile> {
    let output = repo.read(STATUS_ARGS).await.unwrap();
    let records: Vec<&str> = output.split('\0').collect();
    let mut files = Vec::new();
    let mut i = 0;
    while i < records.len() {
        let record = records[i];
        match record.chars().next() {
            // Ordinary change: 1 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <path>
            Some('1') => {
                let fields = split_fields(record, 9);
                files.push(file(fields[8], None, fields[1]));
            }
            // Rename or copy: 2 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <X><score> <path>, then
            // <origPath>
            Some('2') => {
                let fields = split_fields(record, 10);
                i += 1;
                let orig_path = records.get(i).copied();
                files.push(file(fields[9], orig_path, fields[1]));
            }
            // Unmerged: u <XY> <sub> <m1> <m2> <m3> <mW> <h1> <h2> <h3> <path>
            Some('u') => {
                let fields = split_fields(record, 11);
                files.push(StatusFile::conflicted(
                    fields[10], fields[1], fields[4], fields[5],
                ));
            }
            Some('?') => files.push(StatusFile::new(
                &record[2..],
                None,
                None,
                Some(FileStatus::Untracked),
            )),
            _ => {}
        }
        i += 1;
    }
    files
}

fn code(code: Option<char>) -> Option<FileStatus> {
    match code? {
        'M' => Some(FileStatus::Modified),
        'A' => Some(FileStatus::Added),
        'D' => Some(FileStatus::Deleted),
        'R' => Some(FileStatus::Renamed),
        'C' => Some(FileStatus::Copied),
        'T' => Some(FileStatus::Typechange),
        _ => None,
    }
}

fn file(path: &str, orig_path: Option<&str>, xy: &str) -> StatusFile {
    let mut codes = xy.chars();
    StatusFile::new(path, orig_path, code(codes.next()), code(codes.next()))
}

/// Splits on spaces into at most `count` fields; the last one keeps the rest (paths may contain
/// spaces).
fn split_fields(record: &str, count: usize) -> Vec<&str> {
    record.splitn(count, ' ').collect()
}
