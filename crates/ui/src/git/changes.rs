//! Ported from `git/changes.ts`.

use gitto_git::FileStatus;
use gitto_git::features::diff::schema::ChangedFile;

use super::diff_source::DiffSource;

/// Lines added and removed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LineCounts {
    pub additions: u64,
    pub deletions: u64,
}

/// Lines added and deleted across `files`; binary files don't count.
pub fn line_totals(files: &[ChangedFile]) -> LineCounts {
    files
        .iter()
        .fold(LineCounts::default(), |sum, file| LineCounts {
            additions: sum.additions + file.additions.unwrap_or(0),
            deletions: sum.deletions + file.deletions.unwrap_or(0),
        })
}

/// Paths to stage or unstage for files; renames need their old path too, for the deletion side.
pub fn staging_paths(files: &[ChangedFile]) -> Vec<String> {
    let mut paths = Vec::with_capacity(files.len());
    for file in files {
        paths.push(file.path.clone());
        if let Some(orig_path) = file.orig_path.as_ref().filter(|path| !path.is_empty()) {
            paths.push(orig_path.clone());
        }
    }
    paths
}

/// A file's line counts, as its list has them; `None` if they weren't counted.
pub fn counts_of(file: &ChangedFile) -> Option<LineCounts> {
    file.additions.map(|additions| LineCounts {
        additions,
        deletions: file.deletions.unwrap_or(0),
    })
}

/// How many lines changed in all: added and removed.
pub fn total_lines(counts: LineCounts) -> u64 {
    counts.additions + counts.deletions
}

/// Whether `file`'s list says it's binary: it has no line counts, though its list does
/// (`uncounted` says whether it counted any).
pub fn is_known_binary(file: &ChangedFile, uncounted: bool) -> bool {
    file.additions.is_none() && !uncounted && file.status != FileStatus::Untracked
}

/// Whether `file` has changes to show as a patch, as far as its list can tell: not for one that's
/// conflicted, binary, or without changed lines, nor for a folder git lists as untracked, which is
/// a repository of its own.
pub fn has_patch(file: &ChangedFile, uncounted: bool) -> bool {
    if file.status == FileStatus::Conflicted || is_nested_repository(file) {
        return false;
    }
    if is_known_binary(file, uncounted) {
        return false;
    }
    counts_of(file).is_none_or(|counts| total_lines(counts) > 0)
}

/// Whether `file` is a repository inside this one: git lists one as an untracked folder.
pub fn is_nested_repository(file: &ChangedFile) -> bool {
    file.status == FileStatus::Untracked && file.path.ends_with('/')
}

/// Why a file without binary contents has no changed lines.
pub fn unchanged_reason(file: &ChangedFile) -> String {
    if let Some(orig_path) = file.orig_path.as_ref().filter(|path| !path.is_empty()) {
        return format!("Renamed from {orig_path}, with the same contents.");
    }
    match file.status {
        FileStatus::Added | FileStatus::Untracked => "An empty file was added.",
        FileStatus::Deleted => "An empty file was deleted.",
        _ => "Only the file's mode changed.",
    }
    .to_owned()
}

/// Which side of the uncommitted changes the file's are, to say next to its name.
pub fn source_label(source: &DiffSource) -> Option<&'static str> {
    match source {
        DiffSource::Unstaged => Some("Unstaged"),
        DiffSource::Staged => Some("Staged"),
        DiffSource::Commit { .. } | DiffSource::Stash { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(
        path: &str,
        status: FileStatus,
        additions: Option<u64>,
        deletions: Option<u64>,
        orig_path: Option<&str>,
    ) -> ChangedFile {
        ChangedFile {
            path: path.into(),
            status,
            orig_path: orig_path.map(str::to_owned),
            additions,
            deletions,
        }
    }

    /// `file(path)` in TS: modified, with a line added.
    fn modified(path: &str) -> ChangedFile {
        file(path, FileStatus::Modified, Some(1), Some(0), None)
    }

    fn uncounted(path: &str, status: FileStatus) -> ChangedFile {
        file(path, status, None, None, None)
    }

    fn empty(path: &str, status: FileStatus) -> ChangedFile {
        file(path, status, Some(0), Some(0), None)
    }

    mod staging_paths {
        use super::*;

        #[test]
        fn stages_a_renamed_file_by_both_its_paths() {
            assert_eq!(
                staging_paths(&[
                    modified("a"),
                    file("b", FileStatus::Renamed, Some(1), Some(0), Some("old"))
                ]),
                ["a", "b", "old"]
            );
        }
    }

    mod is_known_binary {
        use super::*;

        #[test]
        fn is_true_for_a_file_without_counts_in_a_list_that_counted() {
            assert!(is_known_binary(
                &uncounted("a", FileStatus::Modified),
                false
            ));
            assert!(!is_known_binary(&modified("a"), false));
        }

        #[test]
        fn cant_tell_in_a_list_that_didnt_count_nor_for_an_untracked_file() {
            assert!(!is_known_binary(
                &uncounted("a", FileStatus::Modified),
                true
            ));
            assert!(!is_known_binary(
                &uncounted("a", FileStatus::Untracked),
                false
            ));
        }
    }

    mod is_nested_repository {
        use super::*;

        #[test]
        fn is_an_untracked_folder() {
            assert!(is_nested_repository(&uncounted(
                "sub/",
                FileStatus::Untracked
            )));
            assert!(!is_nested_repository(&uncounted(
                "sub/a.txt",
                FileStatus::Untracked
            )));
            assert!(!is_nested_repository(&modified("sub/")));
        }
    }

    mod has_patch {
        use super::*;

        #[test]
        fn is_true_for_a_file_with_changed_lines_or_whose_lines_werent_counted() {
            assert!(has_patch(&modified("a"), false));
            assert!(has_patch(&uncounted("a", FileStatus::Untracked), false));
            assert!(has_patch(&uncounted("a", FileStatus::Modified), true));
        }

        #[test]
        fn is_false_for_a_conflicted_binary_or_unchanged_file_or_a_nested_repository() {
            assert!(!has_patch(
                &file("a", FileStatus::Conflicted, Some(1), Some(0), None),
                false
            ));
            assert!(!has_patch(&uncounted("a", FileStatus::Modified), false));
            assert!(!has_patch(&empty("a", FileStatus::Modified), false));
            assert!(!has_patch(&uncounted("sub/", FileStatus::Untracked), false));
        }
    }

    mod unchanged_reason {
        use super::*;

        #[test]
        fn says_why_there_are_no_changed_lines() {
            assert_eq!(
                unchanged_reason(&file("b", FileStatus::Renamed, Some(0), Some(0), Some("a"))),
                "Renamed from a, with the same contents."
            );
            assert_eq!(
                unchanged_reason(&empty("a", FileStatus::Added)),
                "An empty file was added."
            );
            assert_eq!(
                unchanged_reason(&empty("a", FileStatus::Untracked)),
                "An empty file was added."
            );
            assert_eq!(
                unchanged_reason(&empty("a", FileStatus::Deleted)),
                "An empty file was deleted."
            );
            assert_eq!(
                unchanged_reason(&empty("a", FileStatus::Modified)),
                "Only the file's mode changed."
            );
        }
    }

    #[test]
    fn totals_the_counted_lines() {
        let files = [
            modified("a"),
            file("b", FileStatus::Modified, Some(2), Some(3), None),
            uncounted("c", FileStatus::Modified),
        ];
        assert_eq!(
            line_totals(&files),
            LineCounts {
                additions: 3,
                deletions: 3
            }
        );
        assert_eq!(source_label(&DiffSource::Staged), Some("Staged"));
    }
}
