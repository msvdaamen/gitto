//! What to tell the user when git refuses to merge, rebase or switch over uncommitted changes or
//! untracked files, going by what it printed. Ported from `merge.ts`, where it was shared with
//! `rebase.ts`.

use std::sync::LazyLock;

use regex::Regex;

use crate::core::errors::Result;
use crate::core::repo::GitCommand;

/// Why git refused to merge over uncommitted changes or untracked files, and the files it named.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefusalKind {
    /// Uncommitted changes to files the merge changes.
    Changes,
    /// Staged changes, which git won't merge with at all, unless it fast-forwards; their files
    /// aren't listed (see `parse_refusal`).
    Staged,
    /// Untracked files the merge would overwrite.
    Overwritten,
    /// Untracked files the merge would remove.
    Removed,
    /// Folders the merge replaces, with untracked files in them.
    Folders,
}

/// Why git refused, and the files it listed (see `parse_refusal`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub kind: RefusalKind,
    pub files: Vec<String>,
}

static STAGED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"Your local changes to the following files would be overwritten by merge:\n {2}\S")
        .expect("a valid regex")
});

static CHANGES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"Your local changes to the following files would be overwritten by (merge|checkout):\n",
    )
    .expect("a valid regex")
});

static UNTRACKED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"The following untracked working tree files would be (overwritten|removed) by (merge|checkout):\n",
    )
    .expect("a valid regex")
});

static FOLDERS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"Updating the following directories would lose untracked files in them:\n")
        .expect("a valid regex")
});

/// Which kind of refusal a pattern of `parse_refusal` found, from what it captured.
type KindOf = fn(&regex::Captures) -> RefusalKind;

/// Why git refused to merge over uncommitted changes or untracked files, going by what it printed,
/// with the files it listed, each on a line of its own, after a tab; `None` if it failed
/// otherwise. Staged changes it lists on one line, two spaces in, separated by spaces, which can't
/// be told apart from spaces in their names: those are left for the caller to ask git for. Also
/// what it printed when a rebase couldn't check out the branch to rebase onto, which it says a
/// checkout would overwrite or remove.
pub fn parse_refusal(stderr: &str) -> Option<Refusal> {
    if STAGED.is_match(stderr) {
        return Some(Refusal {
            kind: RefusalKind::Staged,
            files: Vec::new(),
        });
    }
    let patterns: [(&Regex, KindOf); 3] = [
        (&CHANGES, |_| RefusalKind::Changes),
        (&UNTRACKED, |captures| {
            if &captures[1] == "removed" {
                RefusalKind::Removed
            } else {
                RefusalKind::Overwritten
            }
        }),
        (&FOLDERS, |_| RefusalKind::Folders),
    ];
    for (pattern, kind) in patterns {
        let Some(captures) = pattern.captures(stderr) else {
            continue;
        };
        let end = captures.get(0).map_or(0, |found| found.end());
        let files: Vec<String> = stderr[end..]
            .split('\n')
            .map_while(|line| line.strip_prefix('\t'))
            .map(str::to_owned)
            .collect();
        if !files.is_empty() {
            return Some(Refusal {
                kind: kind(&captures),
                files,
            });
        }
    }
    None
}

/// How a refusal is put to the user (see `refusal_message`).
#[derive(Debug, Clone, Copy)]
pub struct Wording<'a> {
    /// What was asked, e.g. "Merging side".
    pub doing: &'a str,
    /// What git won't do, e.g. "merge side".
    pub wont: &'a str,
    /// What to do once that's seen to, e.g. "merge".
    pub next: &'a str,
}

/// What to tell the user when git refused to go on over `files`, for the reason `kind`, worded as
/// `wording` says.
pub fn refusal_message(kind: RefusalKind, files: &[String], wording: &Wording) -> String {
    let Wording { doing, wont, next } = *wording;
    let one = files.len() == 1;
    let them = if one { "it" } else { "them" };
    let files = list(files);
    match kind {
        RefusalKind::Changes => format!(
            "{doing} would overwrite your uncommitted changes to {files}. Commit or stash them, then {next}."
        ),
        RefusalKind::Staged => format!(
            "Git won't {wont} while changes are staged, to {files}. Commit or stash them, then {next}."
        ),
        RefusalKind::Overwritten | RefusalKind::Removed => format!(
            "{doing} would {} the untracked {} {files}. Move or remove {them}, then {next}.",
            if kind == RefusalKind::Removed {
                "remove"
            } else {
                "overwrite"
            },
            if one { "file" } else { "files" },
        ),
        RefusalKind::Folders => format!(
            "{doing} would lose the untracked files in the {} {files}. Move or remove them, then {next}.",
            if one { "folder" } else { "folders" },
        ),
    }
}

/// The files with staged changes, for a refusal over them (see `parse_refusal`).
pub async fn staged_files(run: &GitCommand) -> Result<Vec<String>> {
    Ok(nul_separated(
        &run.run(["diff", "--cached", "--name-only", "-z", "HEAD"])
            .await?,
    ))
}

/// The NUL-separated names in `output`.
pub(crate) fn nul_separated(output: &str) -> Vec<String> {
    output
        .split('\0')
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect()
}

/// `files` in a sentence: "a.txt", "a.txt and b.txt", or "a.txt and 3 other files".
pub fn list(files: &[String]) -> String {
    match files {
        // No caller asks for none: git names the files, or the caller checks it found some. TS
        // words it "undefined and -1 other files".
        [] => "the files".to_owned(),
        [first] => first.clone(),
        [first, second] => format!("{first} and {second}"),
        [first, ..] => format!("{first} and {} other files", files.len() - 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refusal(kind: RefusalKind, files: &[&str]) -> Option<Refusal> {
        Some(Refusal {
            kind,
            files: files.iter().map(|file| (*file).to_owned()).collect(),
        })
    }

    fn names(files: &[&str]) -> Vec<String> {
        files.iter().map(|file| (*file).to_owned()).collect()
    }

    #[test]
    fn is_read_for_each_reason_with_the_files_it_lists() {
        assert_eq!(
            parse_refusal(
                "error: The following untracked working tree files would be removed by merge:\n\tgone.txt\n\t a b \nPlease move or remove them before you merge.\nAborting\n"
            ),
            refusal(RefusalKind::Removed, &["gone.txt", " a b "])
        );
        assert_eq!(
            parse_refusal(
                "error: The following untracked working tree files would be overwritten by merge:\n\tnew.txt\nPlease move or remove them before you merge.\n"
            ),
            refusal(RefusalKind::Overwritten, &["new.txt"])
        );
        assert_eq!(
            parse_refusal(
                "error: Your local changes to the following files would be overwritten by merge:\n\ta.txt\n\tb.txt\nPlease commit your changes or stash them before you merge.\n"
            ),
            refusal(RefusalKind::Changes, &["a.txt", "b.txt"])
        );
        assert_eq!(
            parse_refusal(
                "error: Your local changes to the following files would be overwritten by merge:\n  a.txt b c.txt\nMerge with strategy ort failed.\n"
            ),
            refusal(RefusalKind::Staged, &[])
        );
        assert_eq!(
            parse_refusal(
                "error: Updating the following directories would lose untracked files in them:\n\td\n\nAborting\n"
            ),
            refusal(RefusalKind::Folders, &["d"])
        );
        assert_eq!(
            parse_refusal(
                "error: The following untracked working tree files would be overwritten by checkout:\n\tm.txt\nPlease move or remove them before you switch branches.\nAborting\nerror: could not detach HEAD\n"
            ),
            refusal(RefusalKind::Overwritten, &["m.txt"])
        );
        assert_eq!(
            parse_refusal("fatal: Not possible to fast-forward, aborting.\n"),
            None
        );
    }

    #[test]
    fn is_put_to_the_user() {
        let merging = Wording {
            doing: "Merging side",
            wont: "merge side",
            next: "merge",
        };
        assert_eq!(
            refusal_message(RefusalKind::Removed, &names(&["gone.txt"]), &merging),
            "Merging side would remove the untracked file gone.txt. Move or remove it, then merge."
        );
        assert_eq!(
            refusal_message(RefusalKind::Removed, &names(&["a", "b", "c"]), &merging),
            "Merging side would remove the untracked files a and 2 other files. Move or remove them, then merge."
        );
        assert_eq!(
            refusal_message(RefusalKind::Folders, &names(&["d", "e"]), &merging),
            "Merging side would lose the untracked files in the folders d and e. Move or remove them, then merge."
        );
        assert_eq!(
            refusal_message(RefusalKind::Staged, &names(&["f.txt"]), &merging),
            "Git won't merge side while changes are staged, to f.txt. Commit or stash them, then merge."
        );
        assert_eq!(
            refusal_message(
                RefusalKind::Overwritten,
                &names(&["m.txt"]),
                &Wording {
                    doing: "Rebasing onto side",
                    wont: "rebase",
                    next: "rebase",
                }
            ),
            "Rebasing onto side would overwrite the untracked file m.txt. Move or remove it, then rebase."
        );
    }

    #[test]
    fn lists_files_in_a_sentence() {
        assert_eq!(list(&names(&[])), "the files");
        assert_eq!(list(&names(&["a"])), "a");
        assert_eq!(list(&names(&["a", "b"])), "a and b");
        assert_eq!(list(&names(&["a", "b", "c"])), "a and 2 other files");
    }
}
