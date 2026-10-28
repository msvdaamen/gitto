//! Reads what `git status` prints. Ported from `parse.ts`.

use std::collections::HashMap;

use super::schema::{ConflictKind, Head, Status, StatusConflict, StatusFile};
use crate::schema::FileStatus;

/// `git status --porcelain=v2 -z --branch`; see "Porcelain Format Version 2" in git-status(1).
pub const STATUS_ARGS: &[&str] = &[
    "status",
    "--porcelain=v2",
    "-z",
    "--branch",
    "--untracked-files=all",
];

/// The status an `XY` letter stands for; `None` for `.` (unchanged) and any other.
fn code(letter: Option<char>) -> Option<FileStatus> {
    Some(match letter? {
        'M' => FileStatus::Modified,
        'A' => FileStatus::Added,
        'D' => FileStatus::Deleted,
        'R' => FileStatus::Renamed,
        'C' => FileStatus::Copied,
        'T' => FileStatus::Typechange,
        _ => return None,
    })
}

/// The status `git status` printed (see `STATUS_ARGS`). A record git wouldn't print, e.g. one
/// that's cut short, is left out.
pub fn parse_status(output: &str) -> Status {
    // `# <key> <value>` lines, e.g. `branch.head` → `main`.
    let mut headers: HashMap<&str, &str> = HashMap::new();
    let mut files: Vec<StatusFile> = Vec::new();

    let mut records = output.split('\0');
    while let Some(record) = records.next() {
        let file = match record.as_bytes().first() {
            Some(b'#') => {
                if let Some((key, value)) = record
                    .strip_prefix("# ")
                    .and_then(|header| header.split_once(' '))
                {
                    headers.insert(key, value);
                }
                None
            }
            // Ordinary change: 1 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <path>
            Some(b'1') => fields::<9>(record).map(|[_, xy, .., path]| file(path, None, xy)),
            // Rename or copy: 2 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <X><score> <path>, then
            // <origPath>
            Some(b'2') => {
                let orig_path = records.next().filter(|path| !path.is_empty());
                fields::<10>(record)
                    .zip(orig_path)
                    .map(|([_, xy, .., path], orig_path)| {
                        file(path, Some(orig_path.to_owned()), xy)
                    })
            }
            // Unmerged: u <XY> <sub> <m1> <m2> <m3> <mW> <h1> <h2> <h3> <path>
            Some(b'u') => fields::<11>(record).and_then(|fields| {
                let [_, xy, _, _, ours, theirs, .., path] = fields;
                Some(StatusFile {
                    path: path.to_owned(),
                    orig_path: None,
                    staged: Some(FileStatus::Conflicted),
                    unstaged: Some(FileStatus::Conflicted),
                    conflict: Some(StatusConflict {
                        kind: ConflictKind::from_xy(xy)?,
                        ours: ours.parse().ok()?,
                        theirs: theirs.parse().ok()?,
                    }),
                })
            }),
            Some(b'?') => record
                .strip_prefix("? ")
                .filter(|path| !path.is_empty())
                .map(|path| StatusFile {
                    path: path.to_owned(),
                    orig_path: None,
                    staged: None,
                    unstaged: Some(FileStatus::Untracked),
                    conflict: None,
                }),
            // "!" (ignored) only shows up with --ignored.
            _ => None,
        };
        files.extend(file);
    }

    let header = |key: &str| headers.get(key).copied();
    let (ahead, behind) = ahead_behind(header("branch.ab").unwrap_or("")).unwrap_or((0, 0));
    Status {
        head: to_head(
            header("branch.oid").unwrap_or(""),
            header("branch.head").unwrap_or(""),
        ),
        upstream: header("branch.upstream").map(str::to_owned),
        ahead,
        behind,
        files,
    }
}

/// The counts of a `branch.ab` header, `+<ahead> -<behind>`.
fn ahead_behind(ab: &str) -> Option<(u64, u64)> {
    let (ahead, behind) = ab.strip_prefix('+')?.split_once(" -")?;
    Some((count(ahead)?, count(behind)?))
}

/// A run of digits, as `\d+` has it.
fn count(digits: &str) -> Option<u64> {
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    // More digits than fit are a count all the same (JS's `Number` would round it).
    Some(digits.parse().unwrap_or(u64::MAX))
}

/// HEAD from the `branch.oid` and `branch.head` headers.
fn to_head(oid: &str, name: &str) -> Head {
    if name == "(detached)" {
        return Head::Detached {
            sha: oid.to_owned(),
        };
    }
    if oid == "(initial)" {
        return Head::Unborn {
            name: name.to_owned(),
        };
    }
    Head::Branch {
        name: name.to_owned(),
        sha: oid.to_owned(),
    }
}

fn file(path: &str, orig_path: Option<String>, xy: &str) -> StatusFile {
    let mut letters = xy.chars();
    StatusFile {
        path: path.to_owned(),
        orig_path,
        staged: code(letters.next()),
        unstaged: code(letters.next()),
        conflict: None,
    }
}

/// A record's `N` fields, split on spaces; the last one keeps the rest, as paths may have spaces
/// in them. `None` if it has fewer, or the last (a path) is empty.
fn fields<const N: usize>(record: &str) -> Option<[&str; N]> {
    let mut split = record.splitn(N, ' ');
    let fields: [&str; N] = std::array::from_fn(|_| split.next().unwrap_or(""));
    (!fields[N - 1].is_empty()).then_some(fields)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output() -> String {
        [
            "# branch.oid 1111111111111111111111111111111111111111",
            "# branch.head main",
            "# branch.upstream origin/main",
            "# branch.ab +2 -1",
            "1 .M N... 100644 100644 100644 aaa aaa src/a file.ts",
            "1 A. N... 000000 100644 100644 000 bbb added.ts",
            "2 R. N... 100644 100644 100644 ccc ccc R100 new name.ts",
            "old name.ts",
            "u UU N... 100644 100644 100644 100644 ddd eee fff conflict.ts",
            "? untracked file.txt",
            "",
        ]
        .join("\0")
    }

    fn status_file(
        path: &str,
        orig_path: Option<&str>,
        staged: Option<FileStatus>,
        unstaged: Option<FileStatus>,
    ) -> StatusFile {
        StatusFile {
            path: path.into(),
            orig_path: orig_path.map(Into::into),
            staged,
            unstaged,
            conflict: None,
        }
    }

    #[test]
    fn reads_the_branch_headers() {
        let status = parse_status(&output());
        assert_eq!(
            status.head,
            Head::Branch {
                name: "main".into(),
                sha: "1111111111111111111111111111111111111111".into(),
            }
        );
        assert_eq!(status.upstream.as_deref(), Some("origin/main"));
        assert_eq!(status.ahead, 2);
        assert_eq!(status.behind, 1);
    }

    #[test]
    fn reads_changed_renamed_conflicted_and_untracked_files() {
        use crate::schema::FileMode;
        use FileStatus::*;
        assert_eq!(
            parse_status(&output()).files,
            [
                status_file("src/a file.ts", None, None, Some(Modified)),
                status_file("added.ts", None, Some(Added), None),
                status_file("new name.ts", Some("old name.ts"), Some(Renamed), None),
                StatusFile {
                    path: "conflict.ts".into(),
                    orig_path: None,
                    staged: Some(Conflicted),
                    unstaged: Some(Conflicted),
                    conflict: Some(StatusConflict {
                        kind: ConflictKind::BothModified,
                        ours: FileMode::Regular,
                        theirs: FileMode::Regular,
                    }),
                },
                status_file("untracked file.txt", None, None, Some(Untracked)),
            ]
        );
    }

    #[test]
    fn handles_a_detached_head() {
        assert_eq!(
            parse_status("# branch.oid 2222\0# branch.head (detached)\0"),
            Status {
                head: Head::Detached { sha: "2222".into() },
                upstream: None,
                ahead: 0,
                behind: 0,
                files: vec![],
            }
        );
    }

    #[test]
    fn handles_a_branch_without_commits() {
        assert_eq!(
            parse_status("# branch.oid (initial)\0# branch.head main\0").head,
            Head::Unborn {
                name: "main".into()
            }
        );
    }

    #[test]
    fn keeps_paths_with_newlines_tabs_and_accents_whole() {
        use FileStatus::*;
        let output = [
            "1 .M N... 100644 100644 100644 aaa aaa two\nlines.txt",
            "2 R. N... 100644 100644 100644 ccc ccc R100 tab\there.txt",
            "was\tthere\n.txt",
            "? café #1.txt",
            "",
        ]
        .join("\0");
        assert_eq!(
            parse_status(&output).files,
            [
                status_file("two\nlines.txt", None, None, Some(Modified)),
                status_file(
                    "tab\there.txt",
                    Some("was\tthere\n.txt"),
                    Some(Renamed),
                    None
                ),
                status_file("café #1.txt", None, None, Some(Untracked)),
            ]
        );
    }

    #[test]
    fn leaves_out_records_git_wouldnt_print() {
        let output = [
            "# branch.head main",
            "#",
            "1 .M N... 100644",
            "u UU N... 100644 100644 100644 100644 ddd eee fff",
            "u XX N... 100644 100644 100644 100644 ddd eee fff odd.ts",
            "u UU N... 100644 040000 100644 100644 ddd eee fff tree.ts",
            "? ",
            "1 .M N... 100644 100644 100644 aaa aaa kept.ts",
            // Cut short: without its previous path.
            "2 R. N... 100644 100644 100644 ccc ccc R100 renamed.ts",
        ]
        .join("\0");
        let status = parse_status(&output);
        assert_eq!(
            status.files,
            [status_file(
                "kept.ts",
                None,
                None,
                Some(FileStatus::Modified)
            )]
        );
        assert_eq!(
            status.head,
            Head::Branch {
                name: "main".into(),
                sha: String::new()
            }
        );
    }
}
