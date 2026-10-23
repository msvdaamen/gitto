//! From `git/changes.ts` (see the module's doc).

use gitto_git::features::diff::schema::ChangedFile;

/// Lines added and deleted.
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
    files
        .iter()
        .flat_map(|file| std::iter::once(file.path.clone()).chain(file.orig_path.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use gitto_git::FileStatus;

    use super::*;

    fn file(path: &str, orig_path: Option<&str>, lines: Option<(u64, u64)>) -> ChangedFile {
        ChangedFile {
            path: path.into(),
            status: FileStatus::Modified,
            orig_path: orig_path.map(Into::into),
            additions: lines.map(|(added, _)| added),
            deletions: lines.map(|(_, deleted)| deleted),
        }
    }

    #[test]
    fn counts_lines_but_not_binary_files() {
        let files = [
            file("a", None, Some((2, 1))),
            file("b", None, None),
            file("c", None, Some((3, 0))),
        ];
        assert_eq!(
            line_totals(&files),
            LineCounts {
                additions: 5,
                deletions: 1
            }
        );
    }

    #[test]
    fn stages_a_rename_by_both_its_paths() {
        let files = [file("new", Some("old"), None), file("a", None, None)];
        assert_eq!(staging_paths(&files), ["new", "old", "a"]);
    }
}
