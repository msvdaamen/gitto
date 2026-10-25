//! The status and the uncommitted changes. Ported from `git/queries/status.ts`.
//!
//! The uncommitted changes come in one load (so the working tree is only walked once), but most
//! of the UI only shows the summary. The file lists, which can be huge, are shared rather than
//! copied (see `super::opaque`), and only split up where they're shown.

use gitto_git::features::diff::schema::ChangedFile;
use gitto_git::features::status::schema::{Head, Uncommitted};
use gitto_git::schema::FileStatus;

use super::opaque::Opaque;

/// The conflicted files, and which of them have no conflict markers left.
#[derive(Debug, Clone, PartialEq)]
pub struct ConflictedFiles {
    pub files: Vec<ChangedFile>,
    pub marker_free: Vec<String>,
}

/// The conflicted files in a status (see [`ConflictedFiles`]).
pub fn select_conflicted(data: &Opaque<Uncommitted>) -> ConflictedFiles {
    ConflictedFiles {
        files: data
            .changes
            .unstaged
            .iter()
            .filter(|file| file.status == FileStatus::Conflicted)
            .cloned()
            .collect(),
        marker_free: data.changes.marker_free.clone(),
    }
}

/// The conflicted file to open first: one with conflict markers left, if any is.
pub fn first_to_resolve(conflicted: &ConflictedFiles) -> Option<&ChangedFile> {
    conflicted
        .files
        .iter()
        .find(|file| !conflicted.marker_free.contains(&file.path))
        .or_else(|| conflicted.files.first())
}

/// The commit HEAD points at; `None` on a branch without commits yet (`headSha`, which
/// `useHeadSha` derived from the status).
pub fn head_sha(head: &Head) -> Option<&str> {
    match head {
        Head::Branch { sha, .. } | Head::Detached { sha } => Some(sha),
        Head::Unborn { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_a_file_with_markers_left_first() {
        let file = |path: &str| -> ChangedFile {
            serde_json::from_value(serde_json::json!({
                "path": path,
                "status": "conflicted",
                "origPath": null,
                "additions": null,
                "deletions": null,
            }))
            .unwrap()
        };
        let conflicted = ConflictedFiles {
            files: vec![file("a.txt"), file("b.txt")],
            marker_free: vec!["a.txt".into()],
        };
        assert_eq!(first_to_resolve(&conflicted).unwrap().path, "b.txt");
        let all_free = ConflictedFiles {
            marker_free: vec!["a.txt".into(), "b.txt".into()],
            ..conflicted
        };
        assert_eq!(first_to_resolve(&all_free).unwrap().path, "a.txt");
    }
}
