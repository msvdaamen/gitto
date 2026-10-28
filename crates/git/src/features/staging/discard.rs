//! Discarding a file's changes, or every change. Ported from `discard.ts`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use futures::future::join_all;
use serde::{Deserialize, Serialize};

use super::commands::{PATHS_FROM_STDIN, unstage_args};
use crate::core::errors::{DiscardBlocker, Error, Result};
use crate::core::git_dirs::git_dirs;
use crate::core::records::{nul_terminated, raw_entries};
use crate::core::repo::{GitCommand, Repo, has_conflicts, ref_exists, succeeds};
use crate::core::runner::RunOptions;
use crate::features::operation::commands::operation_under_way;
use crate::schema::{FileMode, FileStatus};

/// Which of a file's changes are discarded: the ones in the index, or in the working tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiscardSide {
    Staged,
    Unstaged,
}

/// A file's changes to discard: on which side, and the file as its list has it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Discarded {
    pub path: String,
    /// Its previous path, if it's renamed; a copy's is left alone, as that file is still there.
    pub orig_path: Option<String>,
    pub status: FileStatus,
    pub side: DiscardSide,
}

/// A file in the index or in HEAD, at one of the paths whose changes are discarded or below it.
#[derive(Debug, Clone)]
struct Entry {
    path: String,
    mode: FileMode,
    /// Its stage in the index: other than 0 for a conflicted file's sides.
    stage: u8,
}

/// The mode on a line of `ls-files --stage` or `ls-tree -r`.
fn parse_mode(mode: &str) -> Result<FileMode> {
    mode.parse()
        .map_err(|_| Error::Bug(format!("Unexpected mode of a file: {mode}")))
}

/// The files whose changes are discarded by deleting them, as their list has them.
fn deleted_status(status: FileStatus) -> bool {
    matches!(
        status,
        FileStatus::Untracked | FileStatus::Added | FileStatus::Copied | FileStatus::Renamed
    )
}

/// Discards a file's changes on one side of the uncommitted ones, `side`, a renamed one's by both
/// its paths. Its unstaged changes put it back as the index has it, deleting it if it's untracked
/// or only marked to be added (`add -N`). Its staged ones put it back as HEAD has it, on disk too,
/// so its unstaged ones go with them; one HEAD doesn't have is deleted, even if it's ignored.
///
/// Fails with `Error::DiscardBlocked`, discarding nothing: if it's conflicted, as what to keep of
/// that is for resolving it; if it's a submodule or another repository inside this one, whose
/// changes are its own; or if putting it back would also lose another file, like an untracked one
/// at the old path of a rename, or the files in a folder that replaced it. Fails with
/// `Error::RepositoryChanged` if it changed since its list was read, so that another file than the
/// user was told would be deleted, or one they weren't told would be.
pub async fn discard(repo: &Repo, file: &Discarded) -> Result<()> {
    let side = file.side;
    // Git lists a repository inside this one as an untracked folder.
    if file.path.ends_with('/') {
        return Err(Error::DiscardBlocked(DiscardBlocker::Repository {
            path: file.path.clone(),
        }));
    }
    let orig_path = match (&file.status, file.orig_path.as_deref()) {
        (FileStatus::Renamed, Some(orig_path)) if !orig_path.is_empty() => Some(orig_path),
        _ => None,
    };
    let paths: Vec<&str> = match orig_path {
        Some(orig_path) => vec![&file.path, orig_path],
        None => vec![&file.path],
    };
    let paths = &paths;
    repo.exclusive(|run| async move {
        // Not `has_head`, which takes git failing for there being none: the file would be deleted
        // then.
        let has_head = ref_exists(&run, "HEAD").await?;
        let (index, head, placeholders) = futures::try_join!(
            index_entries(&run, paths),
            async {
                if has_head {
                    head_entries(&run, paths).await
                } else {
                    Ok(Vec::new())
                }
            },
            async {
                if side == DiscardSide::Unstaged {
                    intent_to_add(&run, paths).await
                } else {
                    Ok(HashSet::new())
                }
            },
        )?;
        if index.iter().any(|entry| entry.stage != 0) {
            return Err(Error::DiscardBlocked(DiscardBlocker::Conflicted {
                path: file.path.clone(),
            }));
        }
        if index
            .iter()
            .chain(&head)
            .any(|entry| entry.mode == FileMode::Submodule)
        {
            return Err(Error::DiscardBlocked(DiscardBlocker::Submodule {
                path: file.path.clone(),
            }));
        }
        // What the files are put back as: HEAD's for the staged changes, the index's for the
        // unstaged ones, but for a placeholder of a file only marked to be added. Any path without
        // a file there is deleted.
        let kept: Vec<Entry> = match side {
            DiscardSide::Staged => head,
            DiscardSide::Unstaged => index
                .iter()
                .filter(|entry| !placeholders.contains(&entry.path))
                .cloned()
                .collect(),
        };
        let kept_paths: HashSet<&str> = kept.iter().map(|entry| entry.path.as_str()).collect();
        let deleted: Vec<&str> = paths
            .iter()
            .copied()
            .filter(|path| !kept_paths.contains(path))
            .collect();
        let deletes = deleted_status(file.status);
        if deleted.contains(&file.path.as_str()) != deletes
            || orig_path.is_some_and(|orig_path| deleted.contains(&orig_path))
        {
            return Err(Error::RepositoryChanged(format!(
                "{} changed since it was shown, so its changes weren't discarded.",
                file.path
            )));
        }
        let in_index: HashSet<&str> = index.iter().map(|entry| entry.path.as_str()).collect();
        let deleted_set: HashSet<&str> = deleted.iter().copied().collect();
        if let Some(blocker) = in_the_way(repo.path(), &kept, &in_index, &deleted_set).await {
            return Err(Error::DiscardBlocked(DiscardBlocker::WouldLose {
                path: file.path.clone(),
                lost: blocker,
            }));
        }

        // Once it's unstaged, the index has the file as HEAD does, which discarding its unstaged
        // changes then puts it back as.
        if side == DiscardSide::Staged {
            run.run_with(
                unstage_args(has_head),
                RunOptions::default().stdin(nul_terminated(paths)),
            )
            .await?;
        }
        let marked: Vec<&str> = paths
            .iter()
            .copied()
            .filter(|path| placeholders.contains(*path))
            .collect();
        if !marked.is_empty() {
            let mut args = vec!["rm", "--cached", "-q", "--"];
            args.extend(&marked);
            run.run(args).await?;
        }
        // First, as a file deleted here can be in the way of a folder put back. Even if it's
        // ignored: one that was force-added is, once it's unstaged.
        if !deleted.is_empty() {
            let mut args = vec!["clean", "-f", "-x", "-q", "--"];
            args.extend(&deleted);
            run.run(args).await?;
            remove_empty_folders(repo.path(), &deleted).await;
        }
        if !kept.is_empty() {
            let mut args = vec!["restore", "--worktree"];
            args.extend(PATHS_FROM_STDIN);
            let kept_paths: Vec<&str> = kept.iter().map(|entry| entry.path.as_str()).collect();
            run.run_with(
                args,
                RunOptions::default()
                    .stdin(nul_terminated(&kept_paths))
                    .rewrites_files(),
            )
            .await?;
        }
        Ok(())
    })
    .await
}

/// `root` with the `/`-separated `path` under it.
fn join(root: &Path, path: &str) -> PathBuf {
    path.split('/')
        .filter(|part| !part.is_empty())
        .fold(root.to_path_buf(), |joined, part| joined.join(part))
}

/// Deletes the folders that deleting `paths` left empty, up to the repository's root, as git does
/// when it deletes a file: it doesn't keep empty folders, so the files were all there was to them.
async fn remove_empty_folders(root: &Path, paths: &[&str]) {
    join_all(paths.iter().map(|path| async move {
        let parts: Vec<&str> = path.split('/').collect();
        for depth in (1..parts.len()).rev() {
            // Each folder once the one in it is gone.
            if async_fs::remove_dir(join(root, &parts[..depth].join("/")))
                .await
                .is_err()
            {
                // Not empty, or not there: then the folders above it aren't empty either.
                return;
            }
        }
    }))
    .await;
}

/// The index's files at `paths`, or below them.
async fn index_entries(run: &GitCommand, paths: &[&str]) -> Result<Vec<Entry>> {
    let mut args = vec!["ls-files", "--stage", "-z", "--"];
    args.extend(paths);
    let output = run.run(args).await?;
    output
        .split('\0')
        .filter(|line| !line.is_empty())
        .map(|line| {
            // `<mode> <object> <stage>\t<path>`
            let fields = line
                .split_once('\t')
                .and_then(|(info, path)| Some((info.split_once(' ')?, path)));
            let Some(((mode, rest), path)) = fields else {
                return Err(Error::Bug(format!("Unexpected file in the index: {line}")));
            };
            let stage = rest.rsplit(' ').next().unwrap_or_default();
            Ok(Entry {
                path: path.to_owned(),
                mode: parse_mode(mode)?,
                stage: stage
                    .parse()
                    .map_err(|_| Error::Bug(format!("Unexpected file in the index: {line}")))?,
            })
        })
        .collect()
}

/// HEAD's files at `paths`, or below them.
async fn head_entries(run: &GitCommand, paths: &[&str]) -> Result<Vec<Entry>> {
    let mut args = vec!["ls-tree", "-r", "-z", "HEAD", "--"];
    args.extend(paths);
    let output = run.run(args).await?;
    output
        .split('\0')
        .filter(|line| !line.is_empty())
        .map(|line| {
            // `<mode> <type> <object>\t<path>`
            let Some((info, path)) = line.split_once('\t') else {
                return Err(Error::Bug(format!("Unexpected file in HEAD: {line}")));
            };
            Ok(Entry {
                path: path.to_owned(),
                mode: parse_mode(info.split(' ').next().unwrap_or_default())?,
                stage: 0,
            })
        })
        .collect()
}

/// Which of `paths` are only marked to be added (`add -N`): the index has a placeholder for them,
/// rather than their contents, which git compares the working tree to as a file it doesn't have.
async fn intent_to_add(run: &GitCommand, paths: &[&str]) -> Result<HashSet<String>> {
    let mut args = vec!["diff-files", "--raw", "-z", "--"];
    args.extend(paths);
    let output = run.run_bytes(args).await?;
    let mut added = HashSet::new();
    for entry in raw_entries(&output) {
        let entry = entry?;
        if entry.status == b"A" {
            added.insert(String::from_utf8_lossy(entry.path).into_owned());
        }
    }
    Ok(added)
}

/// What's at a path in the working tree, by `lstat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Folder,
    File,
}

/// What putting the files `kept` back in the working tree at `root` would lose, besides the
/// changes discarded, if anything: a folder where one goes, or a file in the way of a folder one
/// goes in; or an untracked file where one goes, which `in_index` doesn't have. Files that are
/// `deleted` anyway don't count.
async fn in_the_way(
    root: &Path,
    kept: &[Entry],
    in_index: &HashSet<&str>,
    deleted: &HashSet<&str>,
) -> Option<String> {
    // Few, and the same folders over and over.
    let mut kinds: HashMap<String, Option<Kind>> = HashMap::new();
    let mut kind = async |path: &str| -> Option<Kind> {
        if let Some(&found) = kinds.get(path) {
            return found;
        }
        let found = async_fs::symlink_metadata(join(root, path))
            .await
            .ok()
            .map(|stats| {
                if stats.is_dir() {
                    Kind::Folder
                } else {
                    Kind::File
                }
            });
        kinds.insert(path.to_owned(), found);
        found
    };
    for entry in kept {
        let path = entry.path.as_str();
        let parts: Vec<&str> = path.split('/').collect();
        for depth in 1..parts.len() {
            let folder = parts[..depth].join("/");
            if !deleted.contains(folder.as_str()) && kind(&folder).await == Some(Kind::File) {
                return Some(format!("the file {folder}"));
            }
        }
        match kind(path).await {
            Some(Kind::Folder) => return Some(format!("what's in the folder {path}")),
            Some(Kind::File) if !in_index.contains(path) => {
                return Some(format!("the untracked file {path}"));
            }
            _ => {}
        }
    }
    None
}

/// Discards every uncommitted change: the index and the working tree are put back as HEAD has
/// them, and untracked files and folders are deleted. Ignored ones are kept, and so are
/// submodules' changes and repositories inside this one. Fails with `Error::DiscardBlocked`,
/// discarding nothing: while an operation is under way, like a merge, which the next commit would
/// finish without its changes; while there are conflicts, which this would resolve; or if there's
/// nothing it would discard, but the changes it keeps.
pub async fn discard_all(repo: &Repo) -> Result<()> {
    let git_dir = git_dirs(repo).await?.git_dir;
    let git_dir = &git_dir;
    repo.exclusive(|run| async move {
        let (operation, conflicted, has_head) = futures::try_join!(
            async { Ok(operation_under_way(git_dir).await) },
            has_conflicts(&run),
            // Not `has_head`, which takes git failing for there being none: every file would be
            // deleted.
            ref_exists(&run, "HEAD"),
        )?;
        if let Some(operation) = operation {
            return Err(Error::DiscardBlocked(DiscardBlocker::OperationUnderWay {
                operation: operation.name().to_owned(),
            }));
        }
        if conflicted {
            return Err(Error::DiscardBlocked(DiscardBlocker::Conflicts));
        }
        if !has_discardable_changes(&run, has_head).await? {
            return Err(Error::DiscardBlocked(DiscardBlocker::NothingToDiscard));
        }
        // Not `reset --hard`, which would also end a merge git left under way in a terminal, say,
        // nor `restore`, which writes every file again, changed or not: in a big repository that's
        // a lot of files touched for nothing.
        let args: &[&str] = if has_head {
            &["read-tree", "-u", "--reset", "HEAD"]
        } else {
            &["rm", "-r", "-f", "-q", "--ignore-unmatch", "--", "."]
        };
        run.run_with(args, RunOptions::default().rewrites_files())
            .await?;
        run.run(["clean", "-f", "-d", "-q"]).await?;
        Ok(())
    })
    .await
}

/// Whether `discard_all` has anything to discard: staged or unstaged changes other than
/// submodules', or untracked files it deletes.
async fn has_discardable_changes(run: &GitCommand, has_head: bool) -> Result<bool> {
    let (staged_none, unstaged_none, untracked) = futures::try_join!(
        async {
            if has_head {
                succeeds(
                    run,
                    [
                        "diff-index",
                        "--cached",
                        "--quiet",
                        "--ignore-submodules",
                        "HEAD",
                    ],
                )
                .await
            } else {
                Ok(run.run(["ls-files"]).await?.is_empty())
            }
        },
        succeeds(run, ["diff-files", "--quiet", "--ignore-submodules"]),
        run.run(["clean", "-n", "-d"]),
    )?;
    Ok(!staged_none || !unstaged_none || !untracked.is_empty())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{Duration, SystemTime};

    use super::*;
    use crate::features::status::schema::StatusFile;
    use crate::schema::FileStatus::*;
    use crate::test_support::conflicts::{Sides, files, try_git};
    use crate::test_support::status::status_entries;
    use crate::test_support::{Fixture, git};

    /// A file's changes to discard, as its list has the file.
    fn discarded(
        side: DiscardSide,
        path: &str,
        status: FileStatus,
        orig_path: Option<&str>,
    ) -> Discarded {
        Discarded {
            path: path.to_owned(),
            orig_path: orig_path.map(str::to_owned),
            status,
            side,
        }
    }

    fn staged(path: &str, status: FileStatus) -> Discarded {
        discarded(DiscardSide::Staged, path, status, None)
    }

    fn unstaged(path: &str, status: FileStatus) -> Discarded {
        discarded(DiscardSide::Unstaged, path, status, None)
    }

    fn entry(path: &str, staged: Option<FileStatus>, unstaged: Option<FileStatus>) -> StatusFile {
        StatusFile::new(path, None, staged, unstaged)
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    fn is_blocked(error: &Error) -> bool {
        matches!(error, Error::DiscardBlocked(_))
    }

    #[test]
    fn discarded_files_are_read_as_the_ui_sends_them() {
        let file: Discarded = serde_json::from_str(
            r#"{"path":"d.txt","origPath":"c.txt","status":"renamed","side":"staged"}"#,
        )
        .unwrap();
        assert_eq!(
            file,
            discarded(DiscardSide::Staged, "d.txt", Renamed, Some("c.txt"))
        );
    }

    // Discarding a file's changes.

    #[test]
    fn puts_a_file_back_as_the_index_has_it_keeping_whats_staged() {
        let fx = Fixture::new();
        let path = fx.create_repo("discard-unstaged");
        fs::write(path.join("f.txt"), "one\n").unwrap();
        git(&path, ["add", "."]);
        git(&path, ["commit", "-qm", "first"]);
        fs::write(path.join("f.txt"), "two\n").unwrap();
        git(&path, ["add", "."]);
        fs::write(path.join("f.txt"), "three\n").unwrap();
        smol::block_on(async {
            let repo = fx.repos.open("discard-unstaged").await.unwrap();

            discard(&repo, &unstaged("f.txt", Modified)).await.unwrap();
            assert_eq!(read(&path.join("f.txt")), "two\n");
            assert_eq!(
                status_entries(&repo).await,
                [entry("f.txt", Some(Modified), None)]
            );
        });
    }

    #[test]
    fn brings_back_a_deleted_file_and_deletes_an_untracked_one_but_no_other() {
        let fx = Fixture::new();
        smol::block_on(async {
            let repo = fx.create_history_repo("discard-untracked").await;
            let path = repo.path().to_path_buf();
            fs::remove_file(path.join("c.txt")).unwrap();
            // Taken as a glob, its name would match the other untracked file too.
            fs::write(path.join("*.txt"), "star\n").unwrap();

            discard(&repo, &unstaged("c.txt", Deleted)).await.unwrap();
            discard(&repo, &unstaged("*.txt", Untracked)).await.unwrap();
            assert_eq!(read(&path.join("c.txt")), "b\n");
            assert!(!path.join("*.txt").exists());
            assert_eq!(
                status_entries(&repo).await,
                [
                    entry("a file.txt", None, Some(Modified)),
                    entry("new file.txt", None, Some(Untracked)),
                ]
            );
        });
    }

    #[test]
    fn puts_a_file_with_staged_changes_back_as_head_has_it_unstaged_changes_and_all() {
        let fx = Fixture::new();
        smol::block_on(async {
            let repo = fx.create_history_repo("discard-staged").await;
            let path = repo.path().to_path_buf();
            git(&path, ["add", "a file.txt"]);
            fs::write(path.join("a file.txt"), "changed again\n").unwrap();
            git(&path, ["add", "new file.txt"]);
            git(&path, ["mv", "c.txt", "d.txt"]);

            discard(&repo, &staged("a file.txt", Modified))
                .await
                .unwrap();
            discard(&repo, &staged("new file.txt", Added))
                .await
                .unwrap();
            discard(
                &repo,
                &discarded(DiscardSide::Staged, "d.txt", Renamed, Some("c.txt")),
            )
            .await
            .unwrap();
            assert_eq!(read(&path.join("a file.txt")), "a\nmore\n");
            // Added, it's deleted, as HEAD doesn't have it; renamed, it's back where it was.
            assert!(!path.join("new file.txt").exists());
            assert!(!path.join("d.txt").exists());
            assert_eq!(read(&path.join("c.txt")), "b\n");
            assert_eq!(status_entries(&repo).await, []);
        });
    }

    #[test]
    fn deletes_a_file_staged_before_the_first_commit() {
        let fx = Fixture::new();
        let path = fx.create_repo("discard-unborn");
        fs::write(path.join("x.txt"), "x\n").unwrap();
        fs::write(path.join("y.txt"), "y\n").unwrap();
        git(&path, ["add", "."]);
        smol::block_on(async {
            let repo = fx.repos.open("discard-unborn").await.unwrap();

            discard(&repo, &staged("x.txt", Added)).await.unwrap();
            assert!(!path.join("x.txt").exists());
            assert_eq!(
                status_entries(&repo).await,
                [entry("y.txt", Some(Added), None)]
            );
        });
    }

    #[test]
    fn deletes_a_file_that_was_force_added_though_its_ignored() {
        let fx = Fixture::new();
        smol::block_on(async {
            let repo = fx.create_history_repo("discard-ignored").await;
            let path = repo.path().to_path_buf();
            fs::write(path.join(".git").join("info").join("exclude"), "*.log\n").unwrap();
            fs::write(path.join("x.log"), "log\n").unwrap();
            git(&path, ["add", "-f", "x.log"]);

            discard(&repo, &staged("x.log", Added)).await.unwrap();
            assert!(!path.join("x.log").exists());
        });
    }

    #[test]
    fn puts_back_a_folder_that_a_file_replaced() {
        let fx = Fixture::new();
        let path = fx.create_repo("discard-folder");
        fs::create_dir(path.join("foo")).unwrap();
        fs::write(path.join("foo").join("bar.txt"), "bar\n").unwrap();
        git(&path, ["add", "."]);
        git(&path, ["commit", "-qm", "first"]);
        fs::remove_dir_all(path.join("foo")).unwrap();
        fs::write(path.join("foo"), "a file\n").unwrap();
        git(&path, ["add", "-A"]);
        smol::block_on(async {
            let repo = fx.repos.open("discard-folder").await.unwrap();

            discard(&repo, &staged("foo", Added)).await.unwrap();
            assert_eq!(read(&path.join("foo").join("bar.txt")), "bar\n");
            assert_eq!(status_entries(&repo).await, []);
        });
    }

    #[test]
    fn deletes_the_folder_a_new_file_leaves_empty_but_not_one_with_other_files_in_it() {
        let fx = Fixture::new();
        smol::block_on(async {
            let repo = fx.create_history_repo("discard-empty-folder").await;
            let path = repo.path().to_path_buf();
            fs::create_dir_all(path.join("deep").join("er")).unwrap();
            fs::write(path.join("deep").join("er").join("x.txt"), "x\n").unwrap();
            git(&path, ["add", "deep"]);
            fs::create_dir(path.join("shared")).unwrap();
            fs::write(path.join("shared").join("kept.txt"), "kept\n").unwrap();
            fs::write(path.join("shared").join("gone.txt"), "gone\n").unwrap();

            discard(&repo, &staged("deep/er/x.txt", Added))
                .await
                .unwrap();
            discard(&repo, &unstaged("shared/gone.txt", Untracked))
                .await
                .unwrap();
            assert!(!path.join("deep").exists());
            assert!(!path.join("shared").join("gone.txt").exists());
            assert_eq!(read(&path.join("shared").join("kept.txt")), "kept\n");
        });
    }

    #[test]
    fn discards_nothing_of_a_submodule_nor_of_a_repository_inside_this_one() {
        let fx = Fixture::new();
        let path = fx.create_repo("discard-submodule");
        fs::write(path.join("f.txt"), "f\n").unwrap();
        git(&path, ["add", "."]);
        git(&path, ["commit", "-qm", "first"]);
        let sha = git(&path, ["rev-parse", "HEAD"]);
        git(
            &path,
            [
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("160000,{sha},sub"),
            ],
        );
        smol::block_on(async {
            let repo = fx.repos.open("discard-submodule").await.unwrap();

            let error = discard(&repo, &staged("sub", Modified)).await.unwrap_err();
            assert!(is_blocked(&error), "{error:?}");
            assert_eq!(
                error.to_string(),
                "sub is a submodule, so its changes weren't discarded. Discard them in the submodule itself."
            );
            let error = discard(&repo, &unstaged("sub", Modified))
                .await
                .unwrap_err();
            assert!(is_blocked(&error), "{error:?}");
            assert!(git(&path, ["ls-files", "--stage", "sub"]).starts_with("160000 "));
            let error = discard(&repo, &unstaged("nested/", Untracked))
                .await
                .unwrap_err();
            assert!(is_blocked(&error), "{error:?}");
        });
    }

    #[test]
    fn deletes_a_file_only_marked_to_be_added_rather_than_emptying_it() {
        let fx = Fixture::new();
        smol::block_on(async {
            let repo = fx.create_history_repo("discard-intent").await;
            let path = repo.path().to_path_buf();
            fs::write(path.join("n.txt"), "content\n").unwrap();
            git(&path, ["add", "-N", "n.txt"]);

            discard(&repo, &unstaged("n.txt", Added)).await.unwrap();
            assert!(!path.join("n.txt").exists());
            assert_eq!(git(&path, ["ls-files", "n.txt"]), "");
        });
    }

    #[test]
    fn deletes_a_copy_leaving_the_file_its_a_copy_of_as_it_is() {
        let fx = Fixture::new();
        let path = fx.create_repo("discard-copy");
        fs::write(path.join("lib.txt"), "lib\n").unwrap();
        git(&path, ["add", "."]);
        git(&path, ["commit", "-qm", "first"]);
        fs::write(path.join("copy.txt"), "lib\n").unwrap();
        fs::write(path.join("lib.txt"), "lib, changed\n").unwrap();
        git(&path, ["add", "."]);
        smol::block_on(async {
            let repo = fx.repos.open("discard-copy").await.unwrap();

            discard(
                &repo,
                &discarded(DiscardSide::Staged, "copy.txt", Copied, Some("lib.txt")),
            )
            .await
            .unwrap();
            assert!(!path.join("copy.txt").exists());
            assert_eq!(read(&path.join("lib.txt")), "lib, changed\n");
            assert_eq!(
                status_entries(&repo).await,
                [entry("lib.txt", Some(Modified), None)]
            );
        });
    }

    #[test]
    fn discards_nothing_that_would_lose_the_files_in_a_folder_that_replaced_it() {
        let fx = Fixture::new();
        let path = fx.create_repo("discard-replaced");
        fs::write(path.join("foo"), "a file\n").unwrap();
        git(&path, ["add", "."]);
        git(&path, ["commit", "-qm", "first"]);
        fs::remove_file(path.join("foo")).unwrap();
        fs::create_dir(path.join("foo")).unwrap();
        fs::write(path.join("foo").join("x"), "x\n").unwrap();
        smol::block_on(async {
            let repo = fx.repos.open("discard-replaced").await.unwrap();

            // Untracked, then staged as added.
            let error = discard(&repo, &unstaged("foo", Deleted)).await.unwrap_err();
            assert!(is_blocked(&error), "{error:?}");
            assert_eq!(
                error.to_string(),
                "foo's changes weren't discarded: putting it back would also lose what's in the folder foo."
            );
            git(&path, ["add", "-A"]);
            let error = discard(&repo, &staged("foo", Deleted)).await.unwrap_err();
            assert!(is_blocked(&error), "{error:?}");
            assert_eq!(read(&path.join("foo").join("x")), "x\n");
            assert_eq!(git(&path, ["ls-files"]), "foo/x");
        });
    }

    #[test]
    fn discards_nothing_of_a_rename_that_would_lose_an_untracked_file_at_its_old_path() {
        let fx = Fixture::new();
        smol::block_on(async {
            let repo = fx.create_history_repo("discard-rename-untracked").await;
            let path = repo.path().to_path_buf();
            git(&path, ["mv", "c.txt", "d.txt"]);
            fs::write(path.join("c.txt"), "another file\n").unwrap();

            let error = discard(
                &repo,
                &discarded(DiscardSide::Staged, "d.txt", Renamed, Some("c.txt")),
            )
            .await
            .unwrap_err();
            assert!(is_blocked(&error), "{error:?}");
            assert_eq!(
                error.to_string(),
                "d.txt's changes weren't discarded: putting it back would also lose the untracked file c.txt."
            );
            assert_eq!(read(&path.join("c.txt")), "another file\n");
            assert!(path.join("d.txt").exists());
        });
    }

    #[test]
    fn discards_nothing_of_a_file_that_changed_since_its_list_was_read() {
        let fx = Fixture::new();
        smol::block_on(async {
            let repo = fx.create_history_repo("discard-changed").await;
            let path = repo.path().to_path_buf();
            // Shown with unstaged changes, then untracked in a terminal: discarding would delete
            // it.
            git(&path, ["rm", "-q", "--cached", "a file.txt"]);

            let error = discard(&repo, &unstaged("a file.txt", Modified))
                .await
                .unwrap_err();
            assert!(matches!(error, Error::RepositoryChanged(_)), "{error:?}");
            assert_eq!(
                error.to_string(),
                "a file.txt changed since it was shown, so its changes weren't discarded."
            );
            assert_eq!(read(&path.join("a file.txt")), "changed\n");
        });
    }

    #[test]
    fn discards_nothing_of_a_folder_with_a_submodule_in_it_in_head() {
        let fx = Fixture::new();
        let path = fx.create_repo("discard-submodule-below");
        fs::write(path.join("f.txt"), "f\n").unwrap();
        git(&path, ["add", "."]);
        git(&path, ["commit", "-qm", "first"]);
        let sha = git(&path, ["rev-parse", "HEAD"]);
        git(
            &path,
            [
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("160000,{sha},foo/sub"),
            ],
        );
        git(&path, ["commit", "-qm", "submodule"]);
        git(&path, ["rm", "-q", "--cached", "foo/sub"]);
        fs::write(path.join("foo"), "a file\n").unwrap();
        git(&path, ["add", "foo"]);
        smol::block_on(async {
            let repo = fx.repos.open("discard-submodule-below").await.unwrap();

            let error = discard(&repo, &staged("foo", Added)).await.unwrap_err();
            assert!(is_blocked(&error), "{error:?}");
            assert!(path.join("foo").exists());
            assert_eq!(git(&path, ["ls-files"]), "f.txt\nfoo");
        });
    }

    #[test]
    fn discards_nothing_of_a_conflicted_file() {
        let fx = Fixture::new();
        let path = fx.create_diverged_repo(
            "discard-conflicted",
            &Sides {
                base: files([("f.txt", "base\n")]),
                ours: files([("f.txt", "ours\n")]),
                theirs: files([("f.txt", "theirs\n")]),
            },
            None,
        );
        assert_ne!(try_git(&path, ["merge", "-q", "side"]), 0);
        let before = read(&path.join("f.txt"));
        smol::block_on(async {
            let repo = fx.repos.open("discard-conflicted").await.unwrap();

            let error = discard(&repo, &staged("f.txt", Conflicted))
                .await
                .unwrap_err();
            assert!(is_blocked(&error), "{error:?}");
            let error = discard(&repo, &unstaged("f.txt", Conflicted))
                .await
                .unwrap_err();
            assert!(is_blocked(&error), "{error:?}");
            assert_eq!(read(&path.join("f.txt")), before);
            assert_ne!(git(&path, ["ls-files", "--unmerged"]), "");
        });
    }

    // Discarding all changes.

    #[test]
    fn puts_back_what_head_has_and_deletes_untracked_files_keeping_ignored_ones() {
        let fx = Fixture::new();
        smol::block_on(async {
            let repo = fx.create_history_repo("discard-all").await;
            let path = repo.path().to_path_buf();
            fs::write(path.join(".gitignore"), "*.log\n").unwrap();
            git(&path, ["add", ".gitignore"]);
            git(&path, ["commit", "-qm", "ignore logs"]);
            git(&path, ["add", "a file.txt", "new file.txt"]);
            fs::write(path.join("a file.txt"), "changed again\n").unwrap();
            git(&path, ["mv", "c.txt", "d.txt"]);
            fs::remove_file(path.join("s.txt")).unwrap();
            fs::create_dir(path.join("folder")).unwrap();
            fs::write(path.join("folder").join("untracked.txt"), "u\n").unwrap();
            fs::write(path.join("debug.log"), "ignored\n").unwrap();
            // An unchanged file isn't written again, once the index knows it's unchanged.
            let long_ago = SystemTime::UNIX_EPOCH + Duration::from_secs(1_577_836_800);
            fs::File::options()
                .write(true)
                .open(path.join("bin.dat"))
                .unwrap()
                .set_times(
                    fs::FileTimes::new()
                        .set_accessed(long_ago)
                        .set_modified(long_ago),
                )
                .unwrap();
            git(&path, ["status"]);

            discard_all(&repo).await.unwrap();
            assert_eq!(status_entries(&repo).await, []);
            assert_eq!(read(&path.join("a file.txt")), "a\nmore\n");
            assert_eq!(read(&path.join("c.txt")), "b\n");
            assert_eq!(read(&path.join("s.txt")), "s\n");
            for gone in ["new file.txt", "d.txt", "folder"] {
                assert!(!path.join(gone).exists(), "{gone}");
            }
            assert!(path.join("debug.log").exists());
            assert_eq!(
                fs::metadata(path.join("bin.dat"))
                    .unwrap()
                    .modified()
                    .unwrap(),
                long_ago
            );
        });
    }

    #[test]
    fn discards_nothing_while_a_merge_is_under_way_finished_or_not() {
        let fx = Fixture::new();
        let path = fx.create_diverged_repo(
            "discard-all-merge",
            &Sides {
                base: files([("f.txt", "base\n"), ("g.txt", "base\n")]),
                ours: files([("f.txt", "ours\n")]),
                theirs: files([("f.txt", "theirs\n"), ("g.txt", "theirs\n")]),
            },
            None,
        );
        assert_ne!(try_git(&path, ["merge", "-q", "side"]), 0);
        fs::write(path.join("new.txt"), "new\n").unwrap();
        smol::block_on(async {
            let repo = fx.repos.open("discard-all-merge").await.unwrap();

            let error = discard_all(&repo).await.unwrap_err();
            assert!(is_blocked(&error), "{error:?}");
            assert_eq!(
                error.to_string(),
                "A merge is under way. Finish or abort it, then discard all changes."
            );
            // Resolved, its commit would record the merge without its changes.
            fs::write(path.join("f.txt"), "resolved\n").unwrap();
            git(&path, ["add", "f.txt"]);
            let error = discard_all(&repo).await.unwrap_err();
            assert!(is_blocked(&error), "{error:?}");
            assert!(path.join("new.txt").exists());
            assert_eq!(read(&path.join("f.txt")), "resolved\n");
            assert_eq!(read(&path.join("g.txt")), "theirs\n");
        });
    }

    #[test]
    fn discards_nothing_while_there_are_conflicts() {
        let fx = Fixture::new();
        let path = fx.create_repo("discard-all-conflicts");
        fs::write(path.join("f.txt"), "base\n").unwrap();
        git(&path, ["add", "."]);
        git(&path, ["commit", "-qm", "base"]);
        fs::write(path.join("f.txt"), "stashed\n").unwrap();
        git(&path, ["stash", "-q"]);
        fs::write(path.join("f.txt"), "committed\n").unwrap();
        git(&path, ["commit", "-qam", "change"]);
        // A pop that conflicts leaves no operation under way.
        assert_ne!(try_git(&path, ["stash", "pop", "-q"]), 0);
        smol::block_on(async {
            let repo = fx.repos.open("discard-all-conflicts").await.unwrap();

            let error = discard_all(&repo).await.unwrap_err();
            assert!(is_blocked(&error), "{error:?}");
            assert_eq!(
                error.to_string(),
                "Some files have conflicts. Resolve them, then discard all changes."
            );
            assert_ne!(git(&path, ["ls-files", "--unmerged"]), "");
        });
    }

    #[test]
    fn discards_nothing_when_theres_nothing_but_submodules_changes() {
        let fx = Fixture::new();
        let path = fx.create_repo("discard-all-submodule");
        fs::write(path.join("f.txt"), "f\n").unwrap();
        git(&path, ["add", "."]);
        git(&path, ["commit", "-qm", "first"]);
        let sha = git(&path, ["rev-parse", "HEAD"]);
        git(
            &path,
            [
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("160000,{sha},sub"),
            ],
        );
        git(&path, ["commit", "-qm", "submodule"]);
        let other = git(&path, ["commit-tree", "HEAD^{tree}", "-m", "other"]);
        git(
            &path,
            [
                "update-index",
                "--cacheinfo",
                &format!("160000,{other},sub"),
            ],
        );
        smol::block_on(async {
            let repo = fx.repos.open("discard-all-submodule").await.unwrap();

            let error = discard_all(&repo).await.unwrap_err();
            assert!(is_blocked(&error), "{error:?}");
            assert_eq!(
                error.to_string(),
                "There's nothing here to discard. A submodule's changes, or those of a repository inside this one, are discarded in it."
            );
            assert!(git(&path, ["ls-files", "--stage", "sub"]).contains(&other));
        });
    }

    #[test]
    fn deletes_every_file_before_the_first_commit() {
        let fx = Fixture::new();
        let path = fx.create_repo("discard-all-unborn");
        fs::write(path.join("staged.txt"), "s\n").unwrap();
        git(&path, ["add", "."]);
        fs::write(path.join("untracked.txt"), "u\n").unwrap();
        smol::block_on(async {
            let repo = fx.repos.open("discard-all-unborn").await.unwrap();

            discard_all(&repo).await.unwrap();
            assert_eq!(status_entries(&repo).await, []);
            assert!(!path.join("staged.txt").exists());
            assert!(!path.join("untracked.txt").exists());

            // Nothing's left to discard.
            let error = discard_all(&repo).await.unwrap_err();
            assert!(is_blocked(&error), "{error:?}");
        });
    }
}
