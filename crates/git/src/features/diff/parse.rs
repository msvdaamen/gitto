//! Reads what git's diff commands print. Ported from `parse.ts`.

use std::collections::HashMap;

use super::schema::ChangedFile;
use crate::schema::FileStatus;

/// The status a raw entry's letter stands for; `None` for one Gitto doesn't know, taken as
/// modified.
fn status_of(code: Option<char>) -> Option<FileStatus> {
    Some(match code? {
        'A' => FileStatus::Added,
        'C' => FileStatus::Copied,
        'D' => FileStatus::Deleted,
        'M' => FileStatus::Modified,
        'R' => FileStatus::Renamed,
        'T' => FileStatus::Typechange,
        'U' => FileStatus::Conflicted,
        _ => return None,
    })
}

/// Parses `--raw --numstat -z` output: first a raw entry per file, then a numstat entry per file.
///
/// - raw: `:<mode> <mode> <sha> <sha> <status>\0<path>\0`, or for renames and copies
///   `:<mode> <mode> <sha> <sha> R<score>\0<origPath>\0<path>\0`
/// - numstat: `<added>\t<deleted>\t<path>\0`, or for renames and copies
///   `<added>\t<deleted>\t\0<origPath>\0<path>\0`. Binary files report `-` for both counts.
pub fn parse_diff(output: &str) -> Vec<ChangedFile> {
    let records: Vec<&str> = output.split('\0').collect();
    // In the order they were first listed, as a JS `Map` keeps them.
    let mut files: Vec<ChangedFile> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let record_at = |i: usize| records.get(i).copied().unwrap_or("");

    let mut i = 0;
    while i < records.len() {
        let record = records[i];
        if record.is_empty() {
            i += 1;
            continue;
        }

        if record.starts_with(':') {
            let code = &record[record.rfind(' ').map_or(0, |space| space + 1)..];
            let status = status_of(code.chars().next()).unwrap_or(FileStatus::Modified);
            let orig_path = if matches!(status, FileStatus::Renamed | FileStatus::Copied) {
                i += 1;
                Some(record_at(i).to_owned())
            } else {
                None
            };
            i += 1;
            let path = record_at(i).to_owned();
            // A conflicted file is listed again as modified, compared to one side of the conflict:
            // it stays conflicted, as the status has it.
            let file = ChangedFile {
                path: path.clone(),
                status,
                orig_path,
                additions: None,
                deletions: None,
            };
            match index.get(&path) {
                Some(&at) if files[at].status == FileStatus::Conflicted => {}
                Some(&at) => files[at] = file,
                None => {
                    index.insert(path, files.len());
                    files.push(file);
                }
            }
            i += 1;
            continue;
        }

        let Some(first_tab) = record.find('\t') else {
            i += 1;
            continue;
        };
        let Some(second_tab) = record[first_tab + 1..]
            .find('\t')
            .map(|at| at + first_tab + 1)
        else {
            i += 1;
            continue;
        };

        let mut path = &record[second_tab + 1..];
        if path.is_empty() {
            i += 1; // origPath, already known from the raw entry
            i += 1;
            path = record_at(i);
        }
        if let Some(&at) = index.get(path) {
            let count = |text: &str| {
                if text == "-" { None } else { text.parse().ok() }
            };
            files[at].additions = count(&record[..first_tab]);
            files[at].deletions = count(&record[first_tab + 1..second_tab]);
        }
        i += 1;
    }

    files
}

#[cfg(test)]
mod tests {
    use super::*;

    fn changed(
        path: &str,
        status: FileStatus,
        orig_path: Option<&str>,
        additions: Option<u64>,
        deletions: Option<u64>,
    ) -> ChangedFile {
        ChangedFile {
            path: path.to_owned(),
            status,
            orig_path: orig_path.map(str::to_owned),
            additions,
            deletions,
        }
    }

    #[test]
    fn combines_raw_statuses_with_numstat_counts() {
        let output = [
            ":100644 100644 aaa bbb M",
            "src/a file.ts",
            ":100644 100644 ccc ccc R086",
            "old.ts",
            "new.ts",
            ":000000 100644 000 ddd A",
            "image.png",
            "3\t1\tsrc/a file.ts",
            "2\t0\t",
            "old.ts",
            "new.ts",
            "-\t-\timage.png",
            "",
        ]
        .join("\0");

        assert_eq!(
            parse_diff(&output),
            vec![
                changed(
                    "src/a file.ts",
                    FileStatus::Modified,
                    None,
                    Some(3),
                    Some(1)
                ),
                changed(
                    "new.ts",
                    FileStatus::Renamed,
                    Some("old.ts"),
                    Some(2),
                    Some(0)
                ),
                changed("image.png", FileStatus::Added, None, None, None),
            ]
        );
    }

    #[test]
    fn keeps_a_conflicted_file_conflicted_though_its_listed_as_modified_too() {
        // What `git diff --raw --numstat -z` has for a file with a conflict.
        let output = [
            ":000000 100644 000 000 U",
            "both.txt",
            ":100644 100644 aaa 000 M",
            "both.txt",
            "0\t0\tboth.txt",
            "4\t0\tboth.txt",
            "",
        ]
        .join("\0");

        assert_eq!(
            parse_diff(&output),
            vec![changed(
                "both.txt",
                FileStatus::Conflicted,
                None,
                Some(4),
                Some(0)
            )]
        );
    }
}
