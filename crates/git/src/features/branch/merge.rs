//! Merging a branch into the checked-out one, and what to tell the user when git refuses to.
//! Ported from `merge.ts`.

use std::sync::LazyLock;

use regex::Regex;

use super::commands::{branch_name, is_ref};
use super::schema::MergeOutcome;
use crate::core::errors::{Error, Result};
use crate::core::git_dirs::git_dirs;
use crate::core::repo::{GitCommand, Repo, current_branch, has_conflicts, ref_exists, resolve_ref};
use crate::core::runner::RunOptions;
use crate::features::operation::commands::operation_under_way;
use crate::features::remote::commands::fetched_name;

/// Merges the branch `ref` (a full ref name, local or remote) into the checked-out branch, `into`,
/// the one the user saw: fails with `Error::RepositoryChanged` if another is checked out by now.
/// What it did:
/// - `UpToDate`: the branch has every commit of `ref` already; nothing changed.
/// - `FastForward`: the branch moved to `ref`, having no commits of its own.
/// - `Merged`: a merge commit was made, worded as `git merge` words it ("Merge branch 'feature'").
/// - `Conflicts`: the merge stopped at conflicts, left to resolve and commit (see the operations).
///
/// The user's `merge.ff` setting is kept, as is `merge.log`. It always commits, though, unsquashed,
/// whatever `branch.<name>.mergeOptions` says, and never stashes the uncommitted changes
/// (`merge.autoStash`): changes git won't merge over make it refuse, saying which files to commit or
/// stash; others are kept as they are.
///
/// Fails with `Error::MergeBlocked`, having done nothing, while HEAD is detached, an operation (a
/// merge, a rebase…) or conflicts are under way, `ref` is gone, or git refused over uncommitted
/// changes or untracked files; with `Error::MergeStopped` if a hook turned the merge commit down,
/// which leaves the merge under way, to commit or abort.
pub async fn merge_branch(repo: &Repo, reference: &str, into: &str) -> Result<MergeOutcome> {
    // As the UI's input is checked.
    if into.is_empty() {
        return Err(Error::InvalidInput(
            "Too small: expected string to have >=1 characters".to_owned(),
        ));
    }
    let Some(target) = branch_name(reference) else {
        return Err(Error::MergeBlocked(format!("{reference} isn't a branch.")));
    };
    let git_dir = git_dirs(repo).await?.git_dir;
    // One write, so nothing else changes the branch, or starts another operation, halfway.
    repo.exclusive(|run| async move {
        let commit_and_head = async {
            // That ref itself, rather than git reading `ref` as a revision, like
            // `refs/heads/main~3`.
            let commit = async {
                if is_ref(&run, reference).await? {
                    resolve_ref(&run, reference).await
                } else {
                    Ok(None)
                }
            };
            let (sha, before) = futures::try_join!(commit, resolve_ref(&run, "HEAD"))?;
            // Read alongside the rest, though only needed once it's checked: it all holds up the
            // other writes. None for a branch without commits yet, which just moves to it: nor can
            // git word one.
            let message = match (&sha, &before) {
                (Some(sha), Some(_)) => Some(merge_message(&run, sha, reference).await?),
                _ => None,
            };
            Ok::<_, Error>((sha, before, message))
        };
        let (operation, conflicted, current, (sha, before, message), named) = futures::try_join!(
            async { Ok::<_, Error>(operation_under_way(&git_dir).await) },
            has_conflicts(&run),
            current_branch(&run),
            commit_and_head,
            resolve_ref(&run, &target.name),
        )?;
        if let Some(operation) = operation {
            return Err(Error::MergeBlocked(format!(
                "A {} is under way. Finish or abort it, then merge.",
                operation.name()
            )));
        }
        if conflicted {
            return Err(Error::MergeBlocked(
                "Some files have conflicts. Resolve them, then merge.".to_owned(),
            ));
        }
        let Some(current) = current else {
            return Err(Error::MergeBlocked(
                "HEAD is detached: check out a branch to merge into it.".to_owned(),
            ));
        };
        if current != into {
            return Err(Error::RepositoryChanged(format!(
                "Switched from {into} to {current} meanwhile, so nothing was merged."
            )));
        }
        let Some(sha) = sha else {
            return Err(Error::MergeBlocked(format!(
                "{} no longer exists.",
                target.name
            )));
        };
        if reference == format!("refs/heads/{into}") {
            return Err(Error::MergeBlocked(format!(
                "Can't merge {into} into itself."
            )));
        }

        let mut args = vec![
            "merge",
            "--quiet",
            "--no-edit",
            // Whatever `branch.<name>.mergeOptions` and `merge.autoStash` say (see above).
            "--commit",
            "--no-squash",
            "--no-autostash",
            // Its log, if `merge.log` wants one, is already in the message.
            "--no-log",
            "--cleanup=strip",
        ];
        if let Some(message) = &message {
            args.extend(["-m", message.as_str()]);
        }
        // By its short name, which labels its side of the conflict markers, unless that's another
        // commit's, like a tag's of the same name: then by its full one.
        args.push(if named.as_ref() == Some(&sha) {
            target.name.as_str()
        } else {
            reference
        });
        // It rewrites the files the branch changed.
        let failure = match run
            .run_with(args, RunOptions::default().rewrites_files())
            .await
        {
            Ok(_) => None,
            Err(Error::Git(error)) => Some(error),
            Err(error) => return Err(error),
        };

        if let Some(failure) = failure {
            let (stopped_at_conflicts, merging) =
                futures::try_join!(has_conflicts(&run), ref_exists(&run, "MERGE_HEAD"))?;
            if stopped_at_conflicts {
                return Ok(MergeOutcome::Conflicts);
            }
            // Also when the merge commit couldn't be written, e.g. as signing it failed.
            if merging {
                return Err(Error::MergeStopped(format!(
                    "Merging {} stopped before committing:\n{}\nFix that, then commit the merge, or abort it.",
                    target.name,
                    failure.without_hints().message
                )));
            }
            let Some(refused) = parse_refusal(&failure.stderr) else {
                return Err(Error::Git(failure.without_hints()));
            };
            // Git lists the staged files on one line, which can't be told apart if a name has a
            // space.
            let files = match refused.kind {
                RefusalKind::Staged => staged_files(&run).await?,
                _ => refused.files,
            };
            return Err(Error::MergeBlocked(refusal_message(
                refused.kind,
                &files,
                &Wording {
                    doing: &format!("Merging {}", target.name),
                    wont: &format!("merge {}", target.name),
                    next: "merge",
                },
            )));
        }
        let after = resolve_ref(&run, "HEAD").await?;
        if after == before {
            return Ok(MergeOutcome::UpToDate);
        }
        Ok(if ref_exists(&run, "HEAD^2").await? {
            MergeOutcome::Merged
        } else {
            MergeOutcome::FastForward
        })
    })
    .await
}

/// The merge commit's message, worded as `git merge` would for the branch `ref`, by its full name:
/// "Merge branch 'feature'", or "Merge remote-tracking branch 'origin/feature'", with " into topic"
/// when merging into a branch other than the main one, and the log `merge.log` asks for.
async fn merge_message(run: &GitCommand, sha: &str, reference: &str) -> Result<String> {
    // A line of FETCH_HEAD, as `git merge` makes for what it merges: "of ." for this repository.
    let line = format!("{sha}\t\t{} of .\n", fetched_name(reference));
    let message = run
        .run_with(["fmt-merge-msg"], RunOptions::default().stdin(line))
        .await?;
    Ok(message.trim().to_owned())
}

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
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::features::conflicts::commands::{ShownConflict, get_conflict, keep_side};
    use crate::features::conflicts::schema::Side;
    use crate::features::operation::commands::{
        abort_operation, continue_operation, get_operation,
    };
    use crate::features::operation::schema::{Operation, OperationKind};
    use crate::features::stash::commands::pop_stash;
    use crate::test_support::conflicts::{
        Files, Sides, commit_files, files, resolve_text, text_conflict, try_git,
    };
    use crate::test_support::{Fixture, git, write_script};

    /// `side` changed `s.txt`, `main` changed `m.txt`: they merge without conflicts.
    fn clean() -> Sides {
        Sides {
            base: files([("f.txt", "one\n")]),
            ours: files([("m.txt", "main\n")]),
            theirs: files([("s.txt", "side\n")]),
        }
    }

    /// The repository's state that a merge that did nothing leaves as it was.
    #[derive(Debug, PartialEq, Eq)]
    struct Snapshot {
        head: String,
        branch: String,
        status: String,
        merging: bool,
    }

    fn snapshot(path: &Path) -> Snapshot {
        Snapshot {
            head: git(path, ["rev-parse", "HEAD"]),
            branch: git(path, ["branch", "--show-current"]),
            status: git(path, ["status", "--porcelain"]),
            merging: path.join(".git").join("MERGE_HEAD").exists(),
        }
    }

    fn read(path: &Path, file: &str) -> String {
        fs::read_to_string(path.join(file)).unwrap()
    }

    fn merging(merging: &str, into: &str) -> Option<Operation> {
        Some(Operation::Merge {
            merging: merging.to_owned(),
            into: Some(into.to_owned()),
        })
    }

    /// Asserts `error` is an `Error::MergeBlocked` saying `message`.
    fn assert_blocked(error: &Error, message: &str) {
        assert!(matches!(error, Error::MergeBlocked(_)), "{error:?}");
        assert_eq!(error.to_string(), message);
    }

    /// A repository `name` with `f.txt` committed on `main`, and a `side` branch a commit ahead,
    /// changing it to `two`.
    fn create_ahead_repo(fx: &Fixture, name: &str) -> PathBuf {
        let path = fx.create_repo(name);
        commit_files(&path, &files([("f.txt", "one\n")]), "first");
        git(&path, ["checkout", "-qb", "side"]);
        commit_files(&path, &files([("f.txt", "two\n")]), "second");
        git(&path, ["checkout", "-q", "main"]);
        path
    }

    // Merging a branch.

    #[test]
    fn fast_forwards_to_a_branch_thats_ahead() {
        let fx = Fixture::new();
        let path = create_ahead_repo(&fx, "merge-ff");

        smol::block_on(async {
            let repo = fx.repos.open("merge-ff").await.unwrap();
            assert_eq!(
                merge_branch(&repo, "refs/heads/side", "main")
                    .await
                    .unwrap(),
                MergeOutcome::FastForward
            );
        });
        assert_eq!(
            git(&path, ["rev-parse", "main"]),
            git(&path, ["rev-parse", "side"])
        );
        assert_eq!(read(&path, "f.txt"), "two\n");
        assert_eq!(git(&path, ["status", "--porcelain"]), "");
    }

    #[test]
    fn makes_a_merge_commit_for_a_branch_that_diverged_worded_as_git_words_it() {
        let fx = Fixture::new();
        let path = fx.create_diverged_repo("merge-commit", &clean(), None);
        let main = git(&path, ["rev-parse", "main"]);

        smol::block_on(async {
            let repo = fx.repos.open("merge-commit").await.unwrap();
            assert_eq!(
                merge_branch(&repo, "refs/heads/side", "main")
                    .await
                    .unwrap(),
                MergeOutcome::Merged
            );
        });
        assert_eq!(
            git(&path, ["log", "-1", "--format=%B"]),
            "Merge branch 'side'"
        );
        assert_eq!(
            git(&path, ["log", "-1", "--format=%P"]),
            format!("{main} {}", git(&path, ["rev-parse", "side"]))
        );
        assert_eq!(read(&path, "s.txt"), "side\n");
        assert_eq!(git(&path, ["status", "--porcelain"]), "");
    }

    #[test]
    fn says_which_branch_it_merged_into_when_it_isnt_the_main_one() {
        let fx = Fixture::new();
        let path = fx.create_diverged_repo("merge-into", &clean(), None);
        git(&path, ["checkout", "-qb", "topic"]);

        smol::block_on(async {
            let repo = fx.repos.open("merge-into").await.unwrap();
            merge_branch(&repo, "refs/heads/side", "topic")
                .await
                .unwrap();
        });
        assert_eq!(
            git(&path, ["log", "-1", "--format=%s"]),
            "Merge branch 'side' into topic"
        );
        assert_ne!(
            git(&path, ["rev-parse", "main"]),
            git(&path, ["rev-parse", "topic"])
        );
    }

    #[test]
    fn merges_a_remote_branch_as_a_remote_tracking_one() {
        let fx = Fixture::new();
        let upstream = fx.create_diverged_repo("merge-upstream", &clean(), None);
        let path = fx.clone_repo("merge-remote", &upstream);
        commit_files(&path, &files([("c.txt", "clone\n")]), "clone");

        smol::block_on(async {
            let repo = fx.repos.open("merge-remote").await.unwrap();
            assert_eq!(
                merge_branch(&repo, "refs/remotes/origin/side", "main")
                    .await
                    .unwrap(),
                MergeOutcome::Merged
            );
        });
        assert_eq!(
            git(&path, ["log", "-1", "--format=%s"]),
            "Merge remote-tracking branch 'origin/side'"
        );
        assert_eq!(
            git(&path, ["rev-parse", "HEAD^2"]),
            git(&path, ["rev-parse", "origin/side"])
        );
    }

    #[test]
    fn does_nothing_when_the_branch_has_every_commit_already() {
        let fx = Fixture::new();
        let path = fx.create_repo("merge-up-to-date");
        commit_files(&path, &files([("f.txt", "one\n")]), "first");
        git(&path, ["branch", "old"]);
        commit_files(&path, &files([("f.txt", "two\n")]), "second");
        let before = snapshot(&path);

        smol::block_on(async {
            let repo = fx.repos.open("merge-up-to-date").await.unwrap();
            assert_eq!(
                merge_branch(&repo, "refs/heads/old", "main").await.unwrap(),
                MergeOutcome::UpToDate
            );
        });
        assert_eq!(snapshot(&path), before);
    }

    #[test]
    fn fast_forwards_a_branch_without_commits_yet_to_the_one_merged() {
        let fx = Fixture::new();
        let path = fx.create_repo("merge-unborn");
        commit_files(&path, &files([("f.txt", "one\n")]), "first");
        git(&path, ["checkout", "-q", "--orphan", "empty"]);
        git(&path, ["rm", "-rqf", "."]);

        smol::block_on(async {
            let repo = fx.repos.open("merge-unborn").await.unwrap();
            assert_eq!(
                merge_branch(&repo, "refs/heads/main", "empty")
                    .await
                    .unwrap(),
                MergeOutcome::FastForward
            );
        });
        assert_eq!(
            git(&path, ["rev-parse", "empty"]),
            git(&path, ["rev-parse", "main"])
        );
        assert_eq!(read(&path, "f.txt"), "one\n");
    }

    #[test]
    fn merges_the_branch_not_a_tag_of_the_same_name_labelling_its_conflicts_by_full_name() {
        let fx = Fixture::new();
        let path = fx.create_diverged_repo("merge-tag-name", &text_conflict(), None);
        git(&path, ["tag", "side", "main~"]);
        smol::block_on(async {
            let repo = fx.repos.open("merge-tag-name").await.unwrap();

            assert_eq!(
                merge_branch(&repo, "refs/heads/side", "main")
                    .await
                    .unwrap(),
                MergeOutcome::Conflicts
            );
            assert!(read(&path, "f.txt").contains(">>>>>>> refs/heads/side\n"));
            resolve_text(&repo, "one\nboth\nthree\n").await;
            continue_operation(&repo, OperationKind::Merge)
                .await
                .unwrap();
        });
        assert_eq!(
            git(&path, ["rev-parse", "HEAD^2"]),
            git(&path, ["rev-parse", "refs/heads/side"])
        );
        assert_eq!(
            git(&path, ["log", "-1", "--format=%s"]),
            "Merge branch 'side'"
        );
    }

    #[test]
    fn keeps_the_uncommitted_changes_the_merge_doesnt_touch() {
        let fx = Fixture::new();
        let path = fx.create_diverged_repo("merge-keeps-changes", &clean(), None);
        fs::write(path.join("f.txt"), "changed\n").unwrap();
        fs::write(path.join("new.txt"), "new\n").unwrap();

        smol::block_on(async {
            let repo = fx.repos.open("merge-keeps-changes").await.unwrap();
            assert_eq!(
                merge_branch(&repo, "refs/heads/side", "main")
                    .await
                    .unwrap(),
                MergeOutcome::Merged
            );
        });
        assert_eq!(git(&path, ["status", "--porcelain"]), "M f.txt\n?? new.txt");
        assert_eq!(read(&path, "f.txt"), "changed\n");
    }

    // With the user's settings.

    #[test]
    fn makes_a_merge_commit_even_for_a_fast_forward_with_merge_ff_false() {
        let fx = Fixture::new();
        let path = create_ahead_repo(&fx, "merge-no-ff");
        git(&path, ["config", "merge.ff", "false"]);

        smol::block_on(async {
            let repo = fx.repos.open("merge-no-ff").await.unwrap();
            assert_eq!(
                merge_branch(&repo, "refs/heads/side", "main")
                    .await
                    .unwrap(),
                MergeOutcome::Merged
            );
        });
        assert_eq!(
            git(&path, ["log", "-1", "--format=%s"]),
            "Merge branch 'side'"
        );
    }

    #[test]
    fn refuses_a_branch_that_diverged_with_merge_ff_only_doing_nothing() {
        let fx = Fixture::new();
        let path = fx.create_diverged_repo("merge-ff-only", &clean(), None);
        git(&path, ["config", "merge.ff", "only"]);
        let before = snapshot(&path);

        smol::block_on(async {
            let repo = fx.repos.open("merge-ff-only").await.unwrap();
            let error = merge_branch(&repo, "refs/heads/side", "main")
                .await
                .unwrap_err();
            assert!(matches!(error, Error::Git(_)), "{error:?}");
            assert_eq!(
                error.to_string(),
                "fatal: Not possible to fast-forward, aborting."
            );
        });
        assert_eq!(snapshot(&path), before);
    }

    #[test]
    fn adds_the_log_of_whats_merged_to_the_message_with_merge_log() {
        let fx = Fixture::new();
        let path = fx.create_diverged_repo("merge-log", &clean(), None);
        git(&path, ["config", "merge.log", "true"]);

        smol::block_on(async {
            let repo = fx.repos.open("merge-log").await.unwrap();
            merge_branch(&repo, "refs/heads/side", "main")
                .await
                .unwrap();
        });
        assert_eq!(
            git(&path, ["log", "-1", "--format=%B"]),
            "Merge branch 'side'\n\n* side:\n  theirs"
        );
    }

    #[test]
    fn commits_the_merge_unsquashed_whatever_the_branchs_merge_options_say() {
        let fx = Fixture::new();
        let path = fx.create_diverged_repo("merge-options", &clean(), None);
        git(
            &path,
            ["config", "branch.main.mergeOptions", "--squash --no-commit"],
        );

        smol::block_on(async {
            let repo = fx.repos.open("merge-options").await.unwrap();
            assert_eq!(
                merge_branch(&repo, "refs/heads/side", "main")
                    .await
                    .unwrap(),
                MergeOutcome::Merged
            );
            assert_eq!(
                git(&path, ["rev-parse", "HEAD^2"]),
                git(&path, ["rev-parse", "side"])
            );
            assert_eq!(get_operation(&repo).await.unwrap(), None);
        });
    }

    #[test]
    fn doesnt_stash_the_changes_it_would_overwrite_whatever_merge_autostash_says() {
        let fx = Fixture::new();
        let path = fx.create_diverged_repo("merge-autostash", &clean(), None);
        git(&path, ["config", "merge.autoStash", "true"]);
        git(&path, ["checkout", "-q", "side"]);
        commit_files(&path, &files([("m.txt", "side's\n")]), "side's m.txt");
        git(&path, ["checkout", "-q", "main"]);
        fs::write(path.join("m.txt"), "mine\n").unwrap();

        smol::block_on(async {
            let repo = fx.repos.open("merge-autostash").await.unwrap();
            merge_branch(&repo, "refs/heads/side", "main")
                .await
                .unwrap_err();
        });
        assert_eq!(git(&path, ["stash", "list"]), "");
        assert_eq!(read(&path, "m.txt"), "mine\n");
    }

    // At conflicts.

    #[test]
    fn stops_leaving_the_merge_under_way_to_resolve() {
        let fx = Fixture::new();
        let path = fx.create_diverged_repo("merge-conflicts", &text_conflict(), None);
        smol::block_on(async {
            let repo = fx.repos.open("merge-conflicts").await.unwrap();

            assert_eq!(
                merge_branch(&repo, "refs/heads/side", "main")
                    .await
                    .unwrap(),
                MergeOutcome::Conflicts
            );
            assert_eq!(get_operation(&repo).await.unwrap(), merging("side", "main"));
        });
        assert_eq!(
            read(&path, "f.txt"),
            "one\n<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> side\nthree\n"
        );
        assert_eq!(git(&path, ["status", "--porcelain"]), "UU f.txt");
    }

    #[test]
    fn commits_the_merge_with_its_message_once_theyre_resolved() {
        let fx = Fixture::new();
        let path = fx.create_diverged_repo("merge-resolved", &text_conflict(), None);
        let main = git(&path, ["rev-parse", "main"]);
        smol::block_on(async {
            let repo = fx.repos.open("merge-resolved").await.unwrap();

            merge_branch(&repo, "refs/heads/side", "main")
                .await
                .unwrap();
            resolve_text(&repo, "one\nboth\nthree\n").await;
            continue_operation(&repo, OperationKind::Merge)
                .await
                .unwrap();

            assert_eq!(get_operation(&repo).await.unwrap(), None);
        });
        assert_eq!(
            git(&path, ["log", "-1", "--format=%B"]),
            "Merge branch 'side'"
        );
        assert_eq!(
            git(&path, ["log", "-1", "--format=%P"]),
            format!("{main} {}", git(&path, ["rev-parse", "side"]))
        );
        assert_eq!(git(&path, ["show", "HEAD:f.txt"]), "one\nboth\nthree");
    }

    #[test]
    fn commits_the_message_of_a_merge_into_another_branch_and_of_a_remote_one() {
        let fx = Fixture::new();
        let upstream = fx.create_diverged_repo("merge-resolved-upstream", &text_conflict(), None);
        let path = fx.clone_repo("merge-resolved-remote", &upstream);
        git(&path, ["checkout", "-qb", "topic"]);
        smol::block_on(async {
            let repo = fx.repos.open("merge-resolved-remote").await.unwrap();

            assert_eq!(
                merge_branch(&repo, "refs/remotes/origin/side", "topic")
                    .await
                    .unwrap(),
                MergeOutcome::Conflicts
            );
            assert!(read(&path, "f.txt").contains(">>>>>>> origin/side\n"));
            assert_eq!(
                get_operation(&repo).await.unwrap(),
                merging("origin/side", "topic")
            );
            resolve_text(&repo, "one\nboth\nthree\n").await;
            continue_operation(&repo, OperationKind::Merge)
                .await
                .unwrap();
        });
        assert_eq!(
            git(&path, ["log", "-1", "--format=%s"]),
            "Merge remote-tracking branch 'origin/side' into topic"
        );
    }

    #[test]
    fn is_aborted_putting_everything_back_as_it_was() {
        let fx = Fixture::new();
        let path = fx.create_diverged_repo("merge-abort", &text_conflict(), None);
        let before = snapshot(&path);
        smol::block_on(async {
            let repo = fx.repos.open("merge-abort").await.unwrap();

            merge_branch(&repo, "refs/heads/side", "main")
                .await
                .unwrap();
            abort_operation(&repo, OperationKind::Merge).await.unwrap();
        });
        assert_eq!(snapshot(&path), before);
        assert_eq!(read(&path, "f.txt"), "one\nours\nthree\n");
    }

    #[test]
    fn stops_at_a_file_deleted_on_one_side_resolved_by_keeping_a_side_whole() {
        let fx = Fixture::new();
        let sides = Sides {
            base: files([("f.txt", "one\n")]),
            ours: Files::new().without("f.txt"),
            theirs: files([("f.txt", "changed\n")]),
        };
        let path = fx.create_diverged_repo("merge-deleted", &sides, None);
        smol::block_on(async {
            let repo = fx.repos.open("merge-deleted").await.unwrap();

            assert_eq!(
                merge_branch(&repo, "refs/heads/side", "main")
                    .await
                    .unwrap(),
                MergeOutcome::Conflicts
            );
            let conflict = get_conflict(&repo, "f.txt").await.unwrap();
            assert_eq!(conflict.ours, None);
            let shown = ShownConflict {
                sides: conflict.sides(),
                version: None,
            };
            keep_side(&repo, "f.txt", Side::Theirs, &shown)
                .await
                .unwrap();
            continue_operation(&repo, OperationKind::Merge)
                .await
                .unwrap();
        });
        assert_eq!(git(&path, ["show", "HEAD:f.txt"]), "changed");
        assert_eq!(
            git(&path, ["log", "-1", "--format=%s"]),
            "Merge branch 'side'"
        );
    }

    #[test]
    fn stops_at_a_binary_file_changed_on_both_sides() {
        let fx = Fixture::new();
        let sides = Sides {
            base: Files::new().with("b.dat", [0u8, 1]),
            ours: Files::new().with("b.dat", [0u8, 2]),
            theirs: Files::new().with("b.dat", [0u8, 3]),
        };
        fx.create_diverged_repo("merge-binary", &sides, None);
        smol::block_on(async {
            let repo = fx.repos.open("merge-binary").await.unwrap();

            assert_eq!(
                merge_branch(&repo, "refs/heads/side", "main")
                    .await
                    .unwrap(),
                MergeOutcome::Conflicts
            );
            let conflict = get_conflict(&repo, "b.dat").await.unwrap();
            assert!(conflict.ours.is_some());
            assert!(conflict.theirs.is_some());
            assert_eq!(conflict.text, None);
        });
    }

    #[test]
    fn keeps_the_uncommitted_changes_to_other_files() {
        let fx = Fixture::new();
        let mut sides = text_conflict();
        sides.base = sides.base.with("g.txt", "g\n");
        let path = fx.create_diverged_repo("merge-conflicts-changes", &sides, None);
        fs::write(path.join("g.txt"), "changed\n").unwrap();

        smol::block_on(async {
            let repo = fx.repos.open("merge-conflicts-changes").await.unwrap();
            assert_eq!(
                merge_branch(&repo, "refs/heads/side", "main")
                    .await
                    .unwrap(),
                MergeOutcome::Conflicts
            );
        });
        assert_eq!(git(&path, ["status", "--porcelain"]), "UU f.txt\n M g.txt");
    }

    // Refuses, doing nothing,

    #[test]
    fn refuses_over_uncommitted_changes_to_a_file_it_changes() {
        let fx = Fixture::new();
        let path = fx.create_diverged_repo("merge-over-changes", &text_conflict(), None);
        fs::write(path.join("f.txt"), "mine\n").unwrap();
        let before = snapshot(&path);

        smol::block_on(async {
            let repo = fx.repos.open("merge-over-changes").await.unwrap();
            let error = merge_branch(&repo, "refs/heads/side", "main")
                .await
                .unwrap_err();
            assert_blocked(
                &error,
                "Merging side would overwrite your uncommitted changes to f.txt. Commit or stash them, then merge.",
            );
        });
        assert_eq!(snapshot(&path), before);
    }

    #[test]
    fn refuses_over_uncommitted_changes_to_several_files_naming_the_first() {
        let fx = Fixture::new();
        let sides = Sides {
            base: files([("a.txt", "a\n"), ("b.txt", "b\n"), ("c.txt", "c\n")]),
            ours: files([("m.txt", "main\n")]),
            theirs: files([("a.txt", "A\n"), ("b.txt", "B\n"), ("c.txt", "C\n")]),
        };
        let path = fx.create_diverged_repo("merge-over-several", &sides, None);
        for file in ["a.txt", "b.txt", "c.txt"] {
            fs::write(path.join(file), "mine\n").unwrap();
        }

        smol::block_on(async {
            let repo = fx.repos.open("merge-over-several").await.unwrap();
            let error = merge_branch(&repo, "refs/heads/side", "main")
                .await
                .unwrap_err();
            assert!(
                error.to_string().contains(
                    "Merging side would overwrite your uncommitted changes to a.txt and 2 other files. Commit or stash them, then merge."
                ),
                "{error}"
            );
        });
    }

    #[test]
    fn refuses_over_staged_changes_which_git_wont_merge_with_even_to_files_it_doesnt_change() {
        let fx = Fixture::new();
        let path = fx.create_diverged_repo("merge-over-staged", &clean(), None);
        fs::write(path.join("f.txt"), "staged\n").unwrap();
        git(&path, ["add", "f.txt"]);
        let before = snapshot(&path);

        smol::block_on(async {
            let repo = fx.repos.open("merge-over-staged").await.unwrap();
            let error = merge_branch(&repo, "refs/heads/side", "main")
                .await
                .unwrap_err();
            assert!(
                error.to_string().contains(
                    "Git won't merge side while changes are staged, to f.txt. Commit or stash them, then merge."
                ),
                "{error}"
            );
        });
        assert_eq!(snapshot(&path), before);
    }

    #[test]
    fn refuses_over_staged_changes_to_several_files_a_name_with_a_space_among_them() {
        let fx = Fixture::new();
        let sides = Sides {
            base: files([("a b.txt", "a\n"), ("f.txt", "f\n")]),
            ..clean()
        };
        let path = fx.create_diverged_repo("merge-over-staged-several", &sides, None);
        fs::write(path.join("a b.txt"), "staged\n").unwrap();
        fs::write(path.join("f.txt"), "staged\n").unwrap();
        git(&path, ["add", "-A"]);

        smol::block_on(async {
            let repo = fx.repos.open("merge-over-staged-several").await.unwrap();
            let error = merge_branch(&repo, "refs/heads/side", "main")
                .await
                .unwrap_err();
            assert!(
                error.to_string().contains(
                    "Git won't merge side while changes are staged, to a b.txt and f.txt. Commit or stash them, then merge."
                ),
                "{error}"
            );
        });
    }

    #[test]
    fn refuses_over_untracked_files_in_a_folder_it_replaces() {
        let fx = Fixture::new();
        // `d` is a folder on `main`, which `side` makes a file.
        let path = fx.create_repo("merge-over-folder");
        fs::create_dir(path.join("d")).unwrap();
        commit_files(&path, &files([("d/k.txt", "k\n")]), "base");
        git(&path, ["checkout", "-qb", "side"]);
        git(&path, ["rm", "-rq", "d"]);
        commit_files(&path, &files([("d", "now a file\n")]), "d is a file");
        git(&path, ["checkout", "-q", "main"]);
        commit_files(&path, &files([("m.txt", "main\n")]), "ours");
        fs::write(path.join("d").join("untracked.txt"), "mine\n").unwrap();
        let before = snapshot(&path);

        smol::block_on(async {
            let repo = fx.repos.open("merge-over-folder").await.unwrap();
            let error = merge_branch(&repo, "refs/heads/side", "main")
                .await
                .unwrap_err();
            assert!(
                error.to_string().contains(
                    "Merging side would lose the untracked files in the folder d. Move or remove them, then merge."
                ),
                "{error}"
            );
        });
        assert_eq!(snapshot(&path), before);
        assert_eq!(read(&path, "d/untracked.txt"), "mine\n");
    }

    #[test]
    fn refuses_over_an_untracked_file_in_the_way_of_one_it_adds() {
        let fx = Fixture::new();
        let path = fx.create_diverged_repo("merge-over-untracked", &clean(), None);
        fs::write(path.join("s.txt"), "mine\n").unwrap();
        let before = snapshot(&path);

        smol::block_on(async {
            let repo = fx.repos.open("merge-over-untracked").await.unwrap();
            let error = merge_branch(&repo, "refs/heads/side", "main")
                .await
                .unwrap_err();
            assert!(
                error.to_string().contains(
                    "Merging side would overwrite the untracked file s.txt. Move or remove it, then merge."
                ),
                "{error}"
            );
        });
        assert_eq!(snapshot(&path), before);
        assert_eq!(read(&path, "s.txt"), "mine\n");
    }

    #[test]
    fn refuses_over_an_untracked_file_in_the_way_of_a_fast_forward() {
        let fx = Fixture::new();
        let path = fx.create_repo("merge-ff-untracked");
        commit_files(&path, &files([("f.txt", "one\n")]), "first");
        git(&path, ["checkout", "-qb", "side"]);
        commit_files(
            &path,
            &files([("s.txt", "side\n"), ("t.txt", "side\n")]),
            "second",
        );
        git(&path, ["checkout", "-q", "main"]);
        fs::write(path.join("s.txt"), "mine\n").unwrap();
        fs::write(path.join("t.txt"), "mine\n").unwrap();

        smol::block_on(async {
            let repo = fx.repos.open("merge-ff-untracked").await.unwrap();
            let error = merge_branch(&repo, "refs/heads/side", "main")
                .await
                .unwrap_err();
            assert!(
                error.to_string().contains(
                    "Merging side would overwrite the untracked files s.txt and t.txt. Move or remove them, then merge."
                ),
                "{error}"
            );
        });
        assert_ne!(
            git(&path, ["rev-parse", "main"]),
            git(&path, ["rev-parse", "side"])
        );
    }

    #[test]
    fn refuses_while_a_merge_is_under_way() {
        let fx = Fixture::new();
        let path = fx.create_diverged_repo("merge-during-merge", &text_conflict(), None);
        git(&path, ["branch", "other", "main~"]);
        smol::block_on(async {
            let repo = fx.repos.open("merge-during-merge").await.unwrap();
            merge_branch(&repo, "refs/heads/side", "main")
                .await
                .unwrap();
            // Resolved, but not committed yet: still under way.
            resolve_text(&repo, "one\nboth\nthree\n").await;

            let error = merge_branch(&repo, "refs/heads/other", "main")
                .await
                .unwrap_err();
            assert_blocked(
                &error,
                "A merge is under way. Finish or abort it, then merge.",
            );
            assert_eq!(get_operation(&repo).await.unwrap(), merging("side", "main"));
        });
    }

    #[test]
    fn refuses_while_a_rebase_is_under_way() {
        let fx = Fixture::new();
        let path = fx.create_diverged_repo("merge-during-rebase", &text_conflict(), None);
        git(&path, ["branch", "other", "main"]);
        git(&path, ["checkout", "-q", "side"]);
        try_git(&path, ["rebase", "main"]);

        smol::block_on(async {
            let repo = fx.repos.open("merge-during-rebase").await.unwrap();
            let error = merge_branch(&repo, "refs/heads/other", "side")
                .await
                .unwrap_err();
            assert_blocked(
                &error,
                "A rebase is under way. Finish or abort it, then merge.",
            );
        });
    }

    #[test]
    fn refuses_while_a_cherry_pick_is_under_way() {
        let fx = Fixture::new();
        let path = fx.create_diverged_repo("merge-during-pick", &text_conflict(), None);
        try_git(&path, ["cherry-pick", "side"]);

        smol::block_on(async {
            let repo = fx.repos.open("merge-during-pick").await.unwrap();
            let error = merge_branch(&repo, "refs/heads/side", "main")
                .await
                .unwrap_err();
            assert_blocked(
                &error,
                "A cherry-pick is under way. Finish or abort it, then merge.",
            );
        });
    }

    #[test]
    fn refuses_while_a_popped_stashs_conflicts_are_left_to_resolve() {
        let fx = Fixture::new();
        let path = fx.create_diverged_repo("merge-during-pop", &clean(), None);
        fs::write(path.join("f.txt"), "stashed\n").unwrap();
        git(&path, ["stash", "push", "-q"]);
        commit_files(&path, &files([("f.txt", "committed\n")]), "second");
        smol::block_on(async {
            let repo = fx.repos.open("merge-during-pop").await.unwrap();
            let stash = git(&path, ["rev-parse", "refs/stash"]);
            let _ = pop_stash(&repo, &stash).await;
            let before = snapshot(&path);

            let error = merge_branch(&repo, "refs/heads/side", "main")
                .await
                .unwrap_err();
            assert_blocked(
                &error,
                "Some files have conflicts. Resolve them, then merge.",
            );
            assert_eq!(snapshot(&path), before);
        });
    }

    #[test]
    fn refuses_with_head_detached() {
        let fx = Fixture::new();
        let path = fx.create_diverged_repo("merge-detached", &clean(), None);
        git(&path, ["checkout", "-q", "--detach"]);
        let before = snapshot(&path);

        smol::block_on(async {
            let repo = fx.repos.open("merge-detached").await.unwrap();
            let error = merge_branch(&repo, "refs/heads/side", "main")
                .await
                .unwrap_err();
            assert_blocked(
                &error,
                "HEAD is detached: check out a branch to merge into it.",
            );
        });
        assert_eq!(snapshot(&path), before);
    }

    #[test]
    fn refuses_once_another_branch_is_checked_out_than_the_one_the_user_saw() {
        let fx = Fixture::new();
        let path = fx.create_diverged_repo("merge-switched", &clean(), None);
        git(&path, ["checkout", "-qb", "topic"]);
        let before = snapshot(&path);

        smol::block_on(async {
            let repo = fx.repos.open("merge-switched").await.unwrap();
            let error = merge_branch(&repo, "refs/heads/side", "main")
                .await
                .unwrap_err();
            assert!(matches!(error, Error::RepositoryChanged(_)), "{error:?}");
            assert_eq!(
                error.to_string(),
                "Switched from main to topic meanwhile, so nothing was merged."
            );
        });
        assert_eq!(snapshot(&path), before);
    }

    #[test]
    fn refuses_once_the_branch_is_gone() {
        let fx = Fixture::new();
        let path = fx.create_diverged_repo("merge-gone", &clean(), None);
        git(&path, ["branch", "-D", "side"]);

        smol::block_on(async {
            let repo = fx.repos.open("merge-gone").await.unwrap();
            let error = merge_branch(&repo, "refs/heads/side", "main")
                .await
                .unwrap_err();
            assert_blocked(&error, "side no longer exists.");
        });
    }

    #[test]
    fn refuses_for_the_branch_checked_out() {
        let fx = Fixture::new();
        fx.create_diverged_repo("merge-itself", &clean(), None);

        smol::block_on(async {
            let repo = fx.repos.open("merge-itself").await.unwrap();
            let error = merge_branch(&repo, "refs/heads/main", "main")
                .await
                .unwrap_err();
            assert_blocked(&error, "Can't merge main into itself.");
        });
    }

    #[test]
    fn refuses_for_a_revision_rather_than_a_branch() {
        let fx = Fixture::new();
        fx.create_diverged_repo("merge-revision", &clean(), None);

        smol::block_on(async {
            let repo = fx.repos.open("merge-revision").await.unwrap();
            let error = merge_branch(&repo, "refs/heads/side~1", "main")
                .await
                .unwrap_err();
            assert_blocked(&error, "side~1 no longer exists.");
        });
    }

    #[test]
    fn refuses_for_a_ref_that_isnt_a_branch() {
        let fx = Fixture::new();
        let path = fx.create_diverged_repo("merge-not-branch", &clean(), None);
        git(&path, ["tag", "v1", "side"]);

        smol::block_on(async {
            let repo = fx.repos.open("merge-not-branch").await.unwrap();
            let error = merge_branch(&repo, "refs/tags/v1", "main")
                .await
                .unwrap_err();
            assert_blocked(&error, "refs/tags/v1 isn't a branch.");
        });
    }

    #[test]
    fn stops_before_committing_when_the_merge_commit_cant_be_signed_to_commit_later() {
        let fx = Fixture::new();
        let path = fx.create_diverged_repo("merge-unsigned", &clean(), None);
        let gpg = path.join(".git").join("no-gpg");
        write_script(&gpg, "#!/bin/sh\nexit 1\n");
        git(&path, ["config", "commit.gpgSign", "true"]);
        // With gpg, whatever the user's global config signs with (ssh, say): `gpg.program` is gpg's.
        git(&path, ["config", "gpg.format", "openpgp"]);
        git(&path, ["config", "gpg.program", gpg.to_str().unwrap()]);
        smol::block_on(async {
            let repo = fx.repos.open("merge-unsigned").await.unwrap();

            let error = merge_branch(&repo, "refs/heads/side", "main")
                .await
                .unwrap_err();
            assert!(matches!(error, Error::MergeStopped(_)), "{error:?}");
            assert!(
                Regex::new(
                    r"(?s)^Merging side stopped before committing:\n.*failed to write commit object\nFix that, then commit the merge, or abort it\.$"
                )
                .unwrap()
                .is_match(&error.to_string()),
                "{error}"
            );
            // Left under way, rather than its result staged with nothing to say so.
            assert_eq!(get_operation(&repo).await.unwrap(), merging("side", "main"));

            git(&path, ["config", "commit.gpgSign", "false"]);
            continue_operation(&repo, OperationKind::Merge)
                .await
                .unwrap();
        });
        assert_eq!(
            git(&path, ["rev-parse", "HEAD^2"]),
            git(&path, ["rev-parse", "side"])
        );
    }

    #[test]
    fn stops_before_committing_when_a_hook_turns_the_merge_commit_down_to_commit_later() {
        let fx = Fixture::new();
        let path = fx.create_diverged_repo("merge-hook", &clean(), None);
        let hook = path.join(".git").join("hooks").join("pre-merge-commit");
        write_script(&hook, "#!/bin/sh\necho 'Not on Fridays.' >&2\nexit 1\n");
        smol::block_on(async {
            let repo = fx.repos.open("merge-hook").await.unwrap();

            let error = merge_branch(&repo, "refs/heads/side", "main")
                .await
                .unwrap_err();
            assert!(matches!(error, Error::MergeStopped(_)), "{error:?}");
            assert_eq!(
                error.to_string(),
                "Merging side stopped before committing:\nNot on Fridays.\nNot committing merge; use 'git commit' to complete the merge.\nFix that, then commit the merge, or abort it."
            );
            assert_eq!(get_operation(&repo).await.unwrap(), merging("side", "main"));

            write_script(&hook, "#!/bin/sh\nexit 0\n");
            continue_operation(&repo, OperationKind::Merge)
                .await
                .unwrap();
        });
        assert_eq!(
            git(&path, ["log", "-1", "--format=%s"]),
            "Merge branch 'side'"
        );
        assert_eq!(
            git(&path, ["rev-parse", "HEAD^2"]),
            git(&path, ["rev-parse", "side"])
        );
    }

    // What git says when it refuses to merge.

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
