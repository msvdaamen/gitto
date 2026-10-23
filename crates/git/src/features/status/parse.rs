//! Reads what `git status` prints. Ported from `parse.ts`.

use std::collections::HashMap;

use super::schema::{Head, Status, StatusConflict, StatusFile};
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

pub fn parse_status(output: &str) -> Status {
    // `# <key> <value>` lines, e.g. `branch.head` → `main`.
    let mut headers: HashMap<&str, String> = HashMap::new();
    let mut files: Vec<StatusFile> = Vec::new();

    let records: Vec<&str> = output.split('\0').collect();
    let mut i = 0;
    while i < records.len() {
        let record = records[i];
        i += 1;
        let Some(kind) = record.chars().next() else {
            continue;
        };

        match kind {
            '#' => {
                let mut parts = record.split(' ');
                parts.next();
                let key = parts.next().unwrap_or("");
                let value: Vec<&str> = parts.collect();
                headers.insert(key, value.join(" "));
            }
            // Ordinary change: 1 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <path>
            '1' => {
                let fields = split_fields(record, 9);
                files.push(file(field(&fields, 8), None, field(&fields, 1)));
            }
            // Rename or copy: 2 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <X><score> <path>, then
            // <origPath>
            '2' => {
                let fields = split_fields(record, 10);
                let orig_path = records.get(i).map(|path| path.to_string());
                i += 1;
                files.push(file(field(&fields, 9), orig_path, field(&fields, 1)));
            }
            // Unmerged: u <XY> <sub> <m1> <m2> <m3> <mW> <h1> <h2> <h3> <path>
            'u' => {
                let fields = split_fields(record, 11);
                files.push(StatusFile {
                    path: field(&fields, 10).to_owned(),
                    orig_path: None,
                    staged: Some(FileStatus::Conflicted),
                    unstaged: Some(FileStatus::Conflicted),
                    conflict: Some(StatusConflict {
                        xy: field(&fields, 1).to_owned(),
                        ours: field(&fields, 4).to_owned(),
                        theirs: field(&fields, 5).to_owned(),
                    }),
                });
            }
            '?' => files.push(StatusFile {
                path: record.get(2..).unwrap_or("").to_owned(),
                orig_path: None,
                staged: None,
                unstaged: Some(FileStatus::Untracked),
                conflict: None,
            }),
            // "!" (ignored) only shows up with --ignored.
            _ => {}
        }
    }

    let header = |key: &str| headers.get(key).map(String::as_str);
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

/// The field at `index`; empty if the record has fewer.
fn field<'a>(fields: &[&'a str], index: usize) -> &'a str {
    fields.get(index).copied().unwrap_or("")
}

/// Splits on spaces into at most `count` fields; the last one keeps the rest (paths may contain
/// spaces).
fn split_fields(record: &str, count: usize) -> Vec<&str> {
    record.splitn(count, ' ').collect()
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
                        xy: "UU".into(),
                        ours: "100644".into(),
                        theirs: "100644".into(),
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
}
