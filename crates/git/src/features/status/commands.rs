//! Reading the status and the uncommitted changes. Ported from `commands.ts`.

use std::future::Future;
use std::time::Instant;

use futures::channel::oneshot;
use sha1_smol::Sha1;

use super::parse::{STATUS_ARGS, parse_status};
use super::schema::{
    Status, StatusCounts, StatusFile, StatusReply, Unchanged, Uncommitted, WorkingTreeFiles,
};
use crate::core::errors::Result;
use crate::core::repo::Repo;
use crate::features::conflicts::commands::marker_free;
use crate::features::diff::parse::parse_diff;
use crate::features::diff::schema::ChangedFile;
use crate::schema::{FileStatus, Version};

/// The status, and the changed files with their line counts: all the uncommitted changes show.
///
/// The UI is told it has this status (see `Repo::show_status`) as it's asked for, once all of it
/// is read: not if it fails, or the future is dropped first.
pub fn get_status(
    repo: &Repo,
) -> impl Future<Output = Result<Uncommitted>> + Send + 'static + use<> {
    let (shown, snapshot) = oneshot::channel::<String>();
    // It's the UI that asks. Only shown once all of it is read: a status that fails isn't.
    repo.show_status(async move { snapshot.await.ok() });
    let repo = repo.clone();
    async move {
        let (uncommitted, snapshot) = read_status(&repo).await?;
        let _ = shown.send(snapshot);
        Ok(uncommitted)
    }
}

/// The status, as `status.get` answers it: `{ unchanged: true }` when it's of `since`, the
/// `version` of the status the caller already has, so a big list of changes isn't sent (and
/// processed by the UI) again for nothing.
pub async fn get_status_since(repo: &Repo, since: Option<&Version>) -> Result<StatusReply> {
    let status = get_status(repo).await?;
    Ok(if Some(&status.version) == since {
        StatusReply::Unchanged {
            unchanged: Unchanged,
        }
    } else {
        StatusReply::Status(Box::new(status))
    })
}

/// What `git status` reports right now, hashed. The same as an earlier one when HEAD, the index
/// and which files differ from it are: all but the changed files' contents, which the working
/// tree's watcher follows.
pub async fn status_snapshot(repo: &Repo) -> Result<String> {
    Ok(snapshot_of(&repo.read(STATUS_ARGS).await?))
}

fn snapshot_of(output: &str) -> String {
    Sha1::from(output).hexdigest()
}

/// The status, and the snapshot of it (see `status_snapshot`).
async fn read_status(repo: &Repo) -> Result<(Uncommitted, String)> {
    let start = Instant::now();
    let output = repo.read(STATUS_ARGS).await?;
    repo.status_took(start.elapsed());
    let Status {
        head,
        upstream,
        ahead,
        behind,
        files,
    } = parse_status(&output);
    let (working_tree, resolved) = futures::join!(
        get_working_tree_files(repo, &files),
        resolved_conflicts(repo, &files)
    );
    let (changes, outputs) = working_tree?;
    // Everything shown comes from git's output, so the same output means the same status, but for
    // which conflicts have markers left, which comes from the files.
    let mut hash = Sha1::new();
    for part in [&output].into_iter().chain(&outputs).chain(&resolved) {
        hash.update(part.as_bytes());
        hash.update(b"\0");
    }
    let uncommitted = Uncommitted {
        head,
        upstream,
        ahead,
        behind,
        counts: count_files(&files),
        changes: WorkingTreeFiles {
            staged: changes.staged,
            unstaged: changes.unstaged,
            uncounted: changes.uncounted,
            marker_free: resolved,
        },
        version: Version::from(hash.hexdigest()),
    };
    Ok((uncommitted, snapshot_of(&output)))
}

fn count_files(files: &[StatusFile]) -> StatusCounts {
    let mut staged = 0;
    let mut unstaged = 0;
    let mut conflicted = 0;
    for file in files {
        if file.staged == Some(FileStatus::Conflicted) {
            conflicted += 1;
        } else {
            if file.staged.is_some() {
                staged += 1;
            }
            if file.unstaged.is_some() {
                unstaged += 1;
            }
        }
    }
    StatusCounts {
        files: files.len() as u64,
        staged,
        unstaged,
        conflicted,
    }
}

/// Past this many conflicted files, none is read to tell whether it still has conflict markers:
/// that is done on every change in the working tree.
const MAX_CHECKED_CONFLICTS: usize = 200;

/// The conflicted text files in `files` that have no conflict markers left (see `marker_free`):
/// those changed or added on both sides, as files rather than links or submodules.
async fn resolved_conflicts(repo: &Repo, files: &[StatusFile]) -> Vec<String> {
    let text: Vec<String> = files
        .iter()
        .filter(|file| {
            file.conflict.as_ref().is_some_and(|conflict| {
                (conflict.xy == "UU" || conflict.xy == "AA")
                    && is_regular_file(&conflict.ours)
                    && is_regular_file(&conflict.theirs)
            })
        })
        .map(|file| file.path.clone())
        .collect();
    if text.is_empty() || text.len() > MAX_CHECKED_CONFLICTS {
        return Vec::new();
    }
    marker_free(repo, &text).await
}

/// Whether `mode` is a file's, executable or not, rather than a link's or a submodule's.
fn is_regular_file(mode: &str) -> bool {
    mode == "100644" || mode == "100755"
}

/// Past this many characters of paths, the unstaged diff isn't limited to the changed files: the
/// command line would get too long (Windows allows 32k characters, git's own arguments included).
const MAX_PATHSPEC_LENGTH: usize = 16_000;

/// Past this many changed files, their lines aren't counted. Counting means diffing every one of
/// them again whenever any file is saved: 0.2s for 1,000 modified files in vscode, 0.5s for 5,000
/// and 2.2s for 20,000, on top of a status that takes 0.1-0.3s.
pub const MAX_COUNTED_FILES: usize = 1000;

/// `WorkingTreeFiles` but for `marker_free`.
struct Changes {
    staged: Vec<ChangedFile>,
    unstaged: Vec<ChangedFile>,
    uncounted: bool,
}

/// Staged and unstaged changes, with their line counts, for the files `git status` reported, and
/// the output of the diffs they came from. The status already walked the working tree, so this
/// doesn't again: the untracked files come from it, and the unstaged diff only looks at the files
/// it says changed. With more files than `MAX_COUNTED_FILES`, all of it comes from the status,
/// which then has all there is to show.
async fn get_working_tree_files(
    repo: &Repo,
    files: &[StatusFile],
) -> Result<(Changes, Vec<String>)> {
    let untracked: Vec<ChangedFile> = files
        .iter()
        .filter(|file| file.unstaged == Some(FileStatus::Untracked))
        .map(|file| uncounted(file, FileStatus::Untracked))
        .collect();
    if files.len() - untracked.len() > MAX_COUNTED_FILES {
        let mut staged = Vec::new();
        let mut unstaged = Vec::new();
        for file in files {
            if let Some(status) = file.staged {
                staged.push(uncounted(file, status));
            }
            if let Some(status) = file.unstaged
                && status != FileStatus::Untracked
            {
                unstaged.push(uncounted(file, status));
            }
        }
        unstaged.extend(untracked);
        let changes = Changes {
            staged,
            unstaged,
            uncounted: true,
        };
        return Ok((changes, Vec::new()));
    }

    let has_staged = files.iter().any(|file| file.staged.is_some());
    let unstaged_paths: Vec<&str> = files
        .iter()
        .filter(|file| file.unstaged.is_some() && file.unstaged != Some(FileStatus::Untracked))
        .map(|file| file.path.as_str())
        .collect();
    // Each path also takes a space and, on Windows, quotes if it has spaces. Counted in UTF-16
    // code units, as JS counts a string's length.
    let length: usize = unstaged_paths
        .iter()
        .map(|path| path.encode_utf16().count() + 3)
        .sum();
    let mut unstaged_args = vec!["diff", "--raw", "--numstat", "-z"];
    if length <= MAX_PATHSPEC_LENGTH {
        unstaged_args.push("--");
        unstaged_args.extend(&unstaged_paths);
    }

    let (staged, unstaged) = futures::try_join!(
        async {
            if has_staged {
                // Without a HEAD commit, `--cached` diffs the index against the empty tree.
                repo.read(["diff", "--cached", "--raw", "--numstat", "-z", "-M"])
                    .await
            } else {
                Ok(String::new())
            }
        },
        async {
            if unstaged_paths.is_empty() {
                Ok(String::new())
            } else {
                repo.read(&unstaged_args).await
            }
        },
    )?;
    let mut unstaged_files = parse_diff(&unstaged);
    unstaged_files.extend(untracked);
    let changes = Changes {
        staged: parse_diff(&staged),
        unstaged: unstaged_files,
        uncounted: false,
    };
    Ok((changes, vec![staged, unstaged]))
}

/// `file`'s change on one side of the index, as the status has it: without line counts.
fn uncounted(file: &StatusFile, status: FileStatus) -> ChangedFile {
    // Its previous path belongs to the side that moved it.
    let moved = status == FileStatus::Renamed || status == FileStatus::Copied;
    ChangedFile {
        path: file.path.clone(),
        status,
        orig_path: if moved { file.orig_path.clone() } else { None },
        additions: None,
        deletions: None,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::core::repo::Observed;
    use crate::core::runner::lock;
    use crate::features::status::schema::Head;
    use crate::test_support::conflicts::try_git;
    use crate::test_support::{Fixture, git};

    fn changed(
        path: &str,
        status: FileStatus,
        additions: Option<u64>,
        deletions: Option<u64>,
    ) -> ChangedFile {
        ChangedFile {
            path: path.into(),
            status,
            orig_path: None,
            additions,
            deletions,
        }
    }

    #[test]
    fn reads_the_status() {
        let fx = Fixture::new();
        smol::block_on(async {
            let repo = fx.create_history_repo("history").await;
            let status = get_status(&repo).await.unwrap();
            assert!(
                matches!(&status.head, Head::Branch { name, .. } if name == "main"),
                "{:?}",
                status.head
            );
            assert_eq!(status.upstream, None);
            assert_eq!((status.ahead, status.behind), (0, 0));
            assert_eq!(
                status.counts,
                StatusCounts {
                    files: 2,
                    staged: 0,
                    unstaged: 2,
                    conflicted: 0
                }
            );
        });
    }

    #[test]
    fn says_how_long_reading_it_took() {
        let fx = Fixture::new();
        smol::block_on(async {
            let repo = fx.create_history_repo("history").await;
            let took = Arc::new(Mutex::new(Vec::new()));
            let told = Arc::clone(&took);
            let observed = repo.observed(move |what| {
                if let Observed::StatusTook(duration) = what {
                    lock(&told).push(*duration);
                }
            });
            get_status(&observed).await.unwrap();
            assert_eq!(lock(&took).len(), 1);
        });
    }

    #[test]
    fn diffs_the_working_tree_against_the_index() {
        let fx = Fixture::new();
        smol::block_on(async {
            let repo = fx.create_history_repo("history").await;
            assert_eq!(
                get_status(&repo).await.unwrap().changes,
                WorkingTreeFiles {
                    staged: vec![],
                    unstaged: vec![
                        changed("a file.txt", FileStatus::Modified, Some(1), Some(2)),
                        changed("new file.txt", FileStatus::Untracked, None, None),
                    ],
                    uncounted: false,
                    marker_free: vec![],
                }
            );
        });
    }

    #[test]
    fn the_version_stays_the_same_until_something_changes() {
        let fx = Fixture::new();
        smol::block_on(async {
            let repo = fx.create_history_repo("versioned").await;
            let version = get_status(&repo).await.unwrap().version;
            assert_eq!(get_status(&repo).await.unwrap().version, version);

            // Only the line counts change: git status reports the same, but the diff doesn't.
            fs::write(repo.path().join("a file.txt"), "changed\nagain\n").unwrap();
            assert_ne!(get_status(&repo).await.unwrap().version, version);
        });
    }

    #[test]
    fn a_repository_without_commits_has_an_empty_status() {
        let fx = Fixture::new();
        fx.create_repo("empty");
        smol::block_on(async {
            let status = get_status(&fx.repos.open("empty").await.unwrap())
                .await
                .unwrap();
            assert_eq!(
                status.head,
                Head::Unborn {
                    name: "main".into()
                }
            );
            assert_eq!(
                status.counts,
                StatusCounts {
                    files: 0,
                    staged: 0,
                    unstaged: 0,
                    conflicted: 0
                }
            );
            assert_eq!(
                status.changes,
                WorkingTreeFiles {
                    staged: vec![],
                    unstaged: vec![],
                    uncounted: false,
                    marker_free: vec![],
                }
            );
        });
    }

    #[test]
    fn answers_unchanged_for_the_version_the_caller_has() {
        let fx = Fixture::new();
        smol::block_on(async {
            let repo = fx.create_history_repo("since").await;
            let StatusReply::Status(status) = get_status_since(&repo, None).await.unwrap() else {
                panic!("a status");
            };
            assert!(matches!(
                get_status_since(&repo, Some(&status.version))
                    .await
                    .unwrap(),
                StatusReply::Unchanged { .. }
            ));
            assert!(matches!(
                get_status_since(&repo, Some(&"other".into()))
                    .await
                    .unwrap(),
                StatusReply::Status(_)
            ));
        });
    }

    /// What the changed files were before they came from the status: full diffs and ls-files.
    async fn full_diffs(repo: &Repo) -> WorkingTreeFiles {
        let (staged, unstaged, untracked) = futures::try_join!(
            repo.read(["diff", "--cached", "--raw", "--numstat", "-z", "-M"]),
            repo.read(["diff", "--raw", "--numstat", "-z"]),
            repo.read(["ls-files", "--others", "--exclude-standard", "-z"]),
        )
        .unwrap();
        let mut unstaged = parse_diff(&unstaged);
        unstaged.extend(
            untracked
                .split('\0')
                .filter(|path| !path.is_empty())
                .map(|path| changed(path, FileStatus::Untracked, None, None)),
        );
        WorkingTreeFiles {
            staged: parse_diff(&staged),
            unstaged,
            uncounted: false,
            marker_free: vec![],
        }
    }

    /// `files` as the status alone has them: without line counts.
    fn without_counts(files: &[ChangedFile]) -> Vec<ChangedFile> {
        files
            .iter()
            .map(|file| ChangedFile {
                additions: None,
                deletions: None,
                ..file.clone()
            })
            .collect()
    }

    fn paths(files: &[ChangedFile]) -> Vec<&str> {
        files.iter().map(|file| file.path.as_str()).collect()
    }

    #[test]
    fn the_changed_files_are_the_same_as_full_diffs() {
        let fx = Fixture::new();
        let path = fx.create_repo("changes");
        // `:colon.txt` would be pathspec magic, if git didn't take paths literally
        // (GIT_LITERAL_PATHSPECS).
        for name in [
            "keep.txt",
            "edit.txt",
            "both.txt",
            "gone.txt",
            "move.txt",
            ":colon.txt",
        ] {
            fs::write(path.join(name), format!("{name}\n")).unwrap();
        }
        fs::write(path.join("bin.dat"), [0u8, 1, 2]).unwrap();
        git(&path, ["add", "."]);
        git(&path, ["commit", "-q", "-m", "first"]);

        fs::write(path.join("edit.txt"), "changed\n").unwrap();
        fs::write(path.join(":colon.txt"), "changed\n").unwrap();
        fs::write(path.join("both.txt"), "staged\n").unwrap();
        git(&path, ["add", "both.txt"]);
        fs::write(path.join("both.txt"), "staged, then changed again\n").unwrap();
        fs::remove_file(path.join("gone.txt")).unwrap();
        git(&path, ["mv", "move.txt", "moved.txt"]);
        fs::write(path.join("bin.dat"), [3u8, 4, 5]).unwrap();
        fs::write(path.join("new.txt"), "new\n").unwrap();
        smol::block_on(async {
            let repo = fx.repos.open("changes").await.unwrap();

            let changes = get_status(&repo).await.unwrap().changes;
            assert_eq!(changes, full_diffs(&repo).await);
            let unstaged = paths(&changes.unstaged);
            for name in [
                "edit.txt",
                ":colon.txt",
                "both.txt",
                "gone.txt",
                "bin.dat",
                "new.txt",
            ] {
                assert!(unstaged.contains(&name), "{name} in {unstaged:?}");
            }
        });
    }

    #[test]
    fn the_changed_files_are_the_same_as_full_diffs_with_a_merge_conflict() {
        let fx = Fixture::new();
        let path = fx.create_repo("conflict");
        fs::write(path.join("file.txt"), "base\n").unwrap();
        git(&path, ["add", "."]);
        git(&path, ["commit", "-q", "-m", "base"]);
        git(&path, ["checkout", "-q", "-b", "side"]);
        fs::write(path.join("file.txt"), "side\n").unwrap();
        git(&path, ["commit", "-q", "-am", "side"]);
        git(&path, ["checkout", "-q", "main"]);
        fs::write(path.join("file.txt"), "main\n").unwrap();
        git(&path, ["commit", "-q", "-am", "main"]);
        assert_ne!(try_git(&path, ["merge", "-q", "side"]), 0);
        smol::block_on(async {
            let repo = fx.repos.open("conflict").await.unwrap();

            let Uncommitted {
                changes, counts, ..
            } = get_status(&repo).await.unwrap();
            assert_eq!(changes, full_diffs(&repo).await);
            // Conflicted on both sides, as it is when there are too many files to diff.
            assert_eq!(changes.unstaged.len(), 1);
            assert_eq!(changes.unstaged[0].path, "file.txt");
            assert_eq!(changes.unstaged[0].status, FileStatus::Conflicted);
            assert_eq!(changes.unstaged[0].additions, Some(4));
            assert_eq!(changes.staged.len(), 1);
            assert_eq!(changes.staged[0].path, "file.txt");
            assert_eq!(changes.staged[0].status, FileStatus::Conflicted);
            assert_eq!(
                counts,
                StatusCounts {
                    files: 1,
                    staged: 0,
                    unstaged: 0,
                    conflicted: 1
                }
            );
        });
    }

    #[test]
    fn the_changed_files_are_the_same_as_full_diffs_when_there_are_too_many_paths_to_list() {
        let fx = Fixture::new();
        let path = fx.create_repo("many");
        let names: Vec<String> = (0..400)
            .map(|i| format!("{}{i}.txt", "long-file-name-".repeat(3)))
            .collect();
        for name in &names {
            fs::write(path.join(name), "a\n").unwrap();
        }
        git(&path, ["add", "."]);
        git(&path, ["commit", "-q", "-m", "many"]);
        for name in &names {
            fs::write(path.join(name), "b\n").unwrap();
        }
        smol::block_on(async {
            let repo = fx.repos.open("many").await.unwrap();

            let changes = get_status(&repo).await.unwrap().changes;
            assert_eq!(changes.unstaged.len(), 400);
            assert_eq!(changes, full_diffs(&repo).await);
        });
    }

    #[test]
    fn the_changed_files_come_from_the_status_alone_without_line_counts_when_there_are_too_many_to_diff()
     {
        let fx = Fixture::new();
        let path = fx.create_repo("uncounted");
        let names: Vec<String> = (0..MAX_COUNTED_FILES)
            .map(|i| format!("file-{i}.txt"))
            .collect();
        for name in names
            .iter()
            .map(String::as_str)
            .chain(["both.txt", "gone.txt", "move.txt"])
        {
            fs::write(path.join(name), format!("{name}\n")).unwrap();
        }
        fs::write(path.join("bin.dat"), [0u8, 1, 2]).unwrap();
        git(&path, ["add", "."]);
        git(&path, ["commit", "-q", "-m", "first"]);
        smol::block_on(async {
            let repo = fx.repos.open("uncounted").await.unwrap();

            // One short of too many: still counted.
            for name in &names {
                fs::write(path.join(name), "changed\n").unwrap();
            }
            assert_eq!(
                get_status(&repo).await.unwrap().changes,
                full_diffs(&repo).await
            );

            fs::write(path.join("both.txt"), "staged\n").unwrap();
            git(&path, ["add", "both.txt"]);
            fs::write(path.join("both.txt"), "staged, then changed again\n").unwrap();
            fs::remove_file(path.join("gone.txt")).unwrap();
            git(&path, ["mv", "move.txt", "moved.txt"]);
            fs::write(path.join("bin.dat"), [3u8, 4, 5]).unwrap();
            fs::write(path.join("new.txt"), "new\n").unwrap();
            let commands = Arc::new(Mutex::new(Vec::new()));
            let ran = Arc::clone(&commands);
            let watched = repo.observed(move |what| {
                if let Observed::Ran { args, .. } = what {
                    lock(&ran).push(args[0].clone());
                }
            });

            let Uncommitted {
                changes, version, ..
            } = get_status(&watched).await.unwrap();
            assert_eq!(*lock(&commands), ["status"]);
            let diffs = full_diffs(&repo).await;
            assert_eq!(
                changes,
                WorkingTreeFiles {
                    staged: without_counts(&diffs.staged),
                    unstaged: without_counts(&diffs.unstaged),
                    uncounted: true,
                    marker_free: vec![],
                }
            );
            assert_eq!(changes.staged.len(), 2);
            assert_eq!(
                (changes.staged[0].path.as_str(), changes.staged[0].status),
                ("both.txt", FileStatus::Modified)
            );
            assert_eq!(
                (
                    changes.staged[1].path.as_str(),
                    changes.staged[1].status,
                    changes.staged[1].orig_path.as_deref()
                ),
                ("moved.txt", FileStatus::Renamed, Some("move.txt"))
            );

            // Nothing shown depends on the files' contents then, so a change to them isn't a new
            // status.
            fs::write(path.join(&names[0]), "changed again\n").unwrap();
            assert_eq!(get_status(&repo).await.unwrap().version, version);
            git(&path, ["add", &names[0]]);
            assert_ne!(get_status(&repo).await.unwrap().version, version);
        });
    }
}
