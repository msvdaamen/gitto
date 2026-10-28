//! Rebasing the checked-out branch onto another. Ported from `rebase.ts`.

use super::preflight::{self, Checked, preflight};
use super::refusal::{RefusalKind, Wording, list, parse_refusal, refusal_message, staged_files};
use super::schema::RebaseOutcome;
use crate::core::errors::{Error, GitError, Integration, IntegrationBlocker, Result};
use crate::core::git_dirs::git_dirs;
use crate::core::records::split_nul;
use crate::core::repo::{GitCommand, Repo, has_conflicts, resolve_ref};
use crate::core::runner::RunOptions;
use crate::features::operation::commands::rebase_state;

/// Rebases the checked-out branch, `branch`, the one the user saw, onto the branch `ref` (a full
/// ref name, local or remote): replays the commits only it has on top of `ref`'s, one by one.
/// Fails with `Error::RepositoryChanged` if another branch is checked out by now. What it did:
/// - `UpToDate`: the branch was on top of `ref` already; nothing changed.
/// - `FastForward`: the branch moved to `ref`, with no commits of its own to replay, or none that
///   `ref` didn't have already, which git leaves out.
/// - `Rebased`: its commits were replayed on top of `ref`.
/// - `Conflicts`: the rebase stopped at a commit's conflicts, left to resolve and continue (see the
///   operations); it may stop again at the next commit's.
///
/// The user's `rebase.*` settings are kept, like `rebase.rebaseMerges` and `rebase.updateRefs`,
/// except these: it never stashes the uncommitted changes (`rebase.autoStash`), which git would
/// otherwise put back after rebasing, or leave in the stash without a word if they conflict, but
/// refuses while there are any, saying which files to commit or stash; and it doesn't squash
/// `fixup!` and `squash!` commits (`rebase.autoSquash`), whose messages would need an editor Gitto
/// can't open. Untracked files are kept as they are, unless one is in the way of `ref`'s files,
/// which makes it refuse too.
///
/// Fails with `Error::RebaseBlocked`, having done nothing, while HEAD is detached or on a branch
/// without commits yet, an operation (a merge, a rebase…) or conflicts are under way, `ref` is
/// gone, or there are uncommitted changes or untracked files in the way. One that stops partway
/// without conflicts, as a commit couldn't be signed say, is undone, and fails with
/// `Error::RebaseUndone` saying why: git leaves such a commit's changes staged, for the user to
/// commit by hand before it goes on, which Gitto can't do for them. With `Error::RebaseStopped` if
/// it couldn't be undone either, which leaves it under way, to finish in a terminal or abort. With
/// `Error::InvalidInput` if `branch` is empty or `ref` isn't a branch, as the UI's input is
/// checked.
pub async fn rebase_branch(repo: &Repo, reference: &str, branch: &str) -> Result<RebaseOutcome> {
    let target = preflight::target(reference, branch)?;
    let git_dir = git_dirs(repo).await?.git_dir;
    // One write, so nothing else changes the branch, or starts another operation, halfway.
    repo.exclusive(|run| async move {
        let Checked { sha, head: before } = preflight(
            &run,
            &git_dir,
            reference,
            &target,
            branch,
            Integration::Rebase,
        )
        .await?;
        if before.is_none() {
            return Err(Error::RebaseBlocked(IntegrationBlocker::NoCommits {
                branch: branch.to_owned(),
                onto: target.name,
            }));
        }

        let args = [
            "rebase",
            "--quiet",
            // Whatever `rebase.autoStash` and `rebase.autoSquash` say (see above).
            "--no-autostash",
            "--no-autosquash",
            // By its full name, which can't be mistaken for another ref's, like a tag's of the same
            // name.
            reference,
        ];
        // With any message as git prepared it, as no editor can be opened. It rewrites the files
        // the branches changed.
        let options = RunOptions::default()
            .env("GIT_EDITOR", ":")
            .rewrites_files();
        let failure = match run.run_with(args, options).await {
            Ok(_) => None,
            Err(Error::Git(error)) => Some(error),
            Err(error) => return Err(error),
        };

        if let Some(failure) = failure {
            let (stopped_at_conflicts, state) =
                futures::join!(has_conflicts(&run), rebase_state(&git_dir));
            if stopped_at_conflicts? {
                return Ok(RebaseOutcome::Conflicts);
            }
            // Stopped without them: a commit couldn't be written, e.g. as signing it failed.
            // Undone, rather than left to continue (see above).
            if state.rebasing {
                let reason = failure.without_hints().message;
                let undone = run
                    .run_with(
                        ["rebase", "--abort"],
                        RunOptions::default().rewrites_files(),
                    )
                    .await;
                let branch = branch.to_owned();
                let onto = target.name;
                if let Err(abort) = undone {
                    return Err(Error::RebaseStopped {
                        branch,
                        onto,
                        reason: format!(
                            "{reason}\nIt couldn't be undone either:\n{}",
                            abort.without_hints()
                        ),
                    });
                }
                return Err(Error::RebaseUndone {
                    branch,
                    onto,
                    reason,
                });
            }
            if let Some(refused) = why_refused(&run, &failure, &target.name).await? {
                return Err(Error::RebaseBlocked(refused));
            }
            return Err(Error::Git(failure.without_hints()));
        }
        let after = resolve_ref(&run, "HEAD").await?;
        if after == before {
            return Ok(RebaseOutcome::UpToDate);
        }
        Ok(if after.as_ref() == Some(&sha) {
            RebaseOutcome::FastForward
        } else {
            RebaseOutcome::Rebased
        })
    })
    .await
}

/// What to tell the user about `failure`, if git refused to rebase onto `onto` with it, having
/// done nothing: over uncommitted changes, which it won't rebase with at all, naming the files, or
/// untracked files in the way of checking out `onto`; `None` if it failed otherwise.
async fn why_refused(
    run: &GitCommand,
    failure: &GitError,
    onto: &str,
) -> Result<Option<IntegrationBlocker>> {
    if let Some(changes) = parse_uncommitted(&failure.stderr) {
        // Git doesn't name them: it checks the working tree first, then the index.
        let (files, what, to) = match changes {
            Uncommitted::Unstaged => (
                split_nul(&run.run(["diff", "--name-only", "-z"]).await?),
                "there are uncommitted changes",
                " to",
            ),
            Uncommitted::Staged => (staged_files(run).await?, "changes are staged", ", to"),
        };
        let named = if files.is_empty() {
            String::new()
        } else {
            format!("{to} {}", list(&files))
        };
        let message =
            format!("Git won't rebase while {what}{named}. Commit or stash them, then rebase.");
        return Ok(Some(IntegrationBlocker::InTheWay { files, message }));
    }
    let Some(refused) = parse_refusal(&failure.stderr) else {
        return Ok(None);
    };
    // Git lists staged changes without naming their files apart (see `parse_refusal`).
    let files = match refused.kind {
        RefusalKind::Staged => staged_files(run).await?,
        _ => refused.files,
    };
    let message = refusal_message(
        refused.kind,
        &files,
        &Wording {
            doing: &format!("Rebasing onto {onto}"),
            wont: "rebase",
            next: "rebase",
        },
    );
    Ok(Some(IntegrationBlocker::InTheWay { files, message }))
}

/// Which uncommitted changes git refused to rebase over (see `parse_uncommitted`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Uncommitted {
    /// "You have unstaged changes".
    Unstaged,
    /// "Your index contains uncommitted changes".
    Staged,
}

/// Whether git refused to rebase over uncommitted changes, going by what it printed, and which:
/// unstaged ones ("You have unstaged changes") or staged ones ("Your index contains uncommitted
/// changes"); `None` if it failed otherwise.
pub fn parse_uncommitted(stderr: &str) -> Option<Uncommitted> {
    if stderr.contains("cannot rebase: You have unstaged changes.") {
        return Some(Uncommitted::Unstaged);
    }
    if stderr.contains("cannot rebase: Your index contains uncommitted changes.") {
        return Some(Uncommitted::Staged);
    }
    None
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
    use crate::features::operation::schema::{Operation, OperationKind, Steps};
    use crate::features::stash::commands::pop_stash;
    use crate::test_support::conflicts::{
        Files, Sides, commit_files, files, resolve_text, text_conflict, try_git,
    };
    use crate::test_support::{Fixture, git, write_script};

    /// `side` changed `s.txt`, `main` changed `m.txt`: `side` rebases onto `main` without
    /// conflicts.
    fn clean() -> Sides {
        Sides {
            base: files([("f.txt", "one\n")]),
            ours: files([("m.txt", "main\n")]),
            theirs: files([("s.txt", "side\n")]),
        }
    }

    /// A repository like `create_diverged_repo` makes, with `side` checked out, to rebase onto
    /// `main`, and a second commit on `side` if `more` is given, changing `f.txt` again.
    fn create_side_repo(fx: &Fixture, name: &str, sides: &Sides, more: Option<&str>) -> PathBuf {
        let path = fx.create_diverged_repo(name, sides, None);
        git(&path, ["checkout", "-q", "side"]);
        if let Some(more) = more {
            commit_files(&path, &files([("f.txt", more)]), "theirs again");
        }
        path
    }

    /// The repository's state that a rebase that did nothing leaves as it was.
    #[derive(Debug, PartialEq, Eq)]
    struct Snapshot {
        head: String,
        branch: String,
        status: String,
        rebasing: bool,
    }

    fn snapshot(path: &Path) -> Snapshot {
        Snapshot {
            head: git(path, ["rev-parse", "HEAD"]),
            branch: git(path, ["branch", "--show-current"]),
            status: git(path, ["status", "--porcelain"]),
            rebasing: path.join(".git").join("rebase-merge").exists(),
        }
    }

    fn read(path: &Path, file: &str) -> String {
        fs::read_to_string(path.join(file)).unwrap()
    }

    /// Asserts `error` is an `Error::RebaseBlocked` saying `message`.
    fn assert_blocked(error: &Error, message: &str) {
        assert!(matches!(error, Error::RebaseBlocked(_)), "{error:?}");
        assert_eq!(error.to_string(), message);
    }

    fn rebasing_side_onto_main(step: u64, total: u64) -> Option<Operation> {
        Some(Operation::Rebase {
            branch: Some("side".to_owned()),
            onto: Some("main".to_owned()),
            steps: Some(Steps { step, total }),
        })
    }

    /// Whether `operation` is a rebase at `step` of `total`.
    fn at_step(operation: &Option<Operation>, step: u64, total: u64) -> bool {
        matches!(
            operation,
            Some(Operation::Rebase { steps: Some(steps), .. }) if *steps == Steps { step, total }
        )
    }

    /// Whether `operation` is a rebase of `side`.
    fn rebasing_side(operation: &Option<Operation>) -> bool {
        matches!(
            operation,
            Some(Operation::Rebase { branch: Some(branch), .. }) if branch == "side"
        )
    }

    // Rebasing a branch.

    #[test]
    fn replays_the_branchs_commits_on_top_of_the_others() {
        let fx = Fixture::new();
        let path = create_side_repo(&fx, "rebase-replay", &clean(), None);
        commit_files(&path, &files([("t.txt", "side too\n")]), "theirs too");
        let main = git(&path, ["rev-parse", "main"]);

        smol::block_on(async {
            let repo = fx.repos.open("rebase-replay").await.unwrap();
            assert_eq!(
                rebase_branch(&repo, "refs/heads/main", "side")
                    .await
                    .unwrap(),
                RebaseOutcome::Rebased
            );
            assert_eq!(
                git(&path, ["log", "--format=%s", "main..side"]),
                "theirs too\ntheirs"
            );
            assert_eq!(git(&path, ["rev-parse", "side~2"]), main);
            assert_eq!(git(&path, ["branch", "--show-current"]), "side");
            assert_eq!(read(&path, "m.txt"), "main\n");
            assert_eq!(read(&path, "s.txt"), "side\n");
            assert_eq!(git(&path, ["status", "--porcelain"]), "");
            assert_eq!(get_operation(&repo).await.unwrap(), None);
        });
    }

    #[test]
    fn moves_a_branch_without_commits_of_its_own_to_the_other() {
        let fx = Fixture::new();
        let path = fx.create_repo("rebase-ff");
        commit_files(&path, &files([("f.txt", "one\n")]), "first");
        git(&path, ["checkout", "-qb", "side"]);
        git(&path, ["checkout", "-q", "main"]);
        commit_files(&path, &files([("f.txt", "two\n")]), "second");
        git(&path, ["checkout", "-q", "side"]);

        smol::block_on(async {
            let repo = fx.repos.open("rebase-ff").await.unwrap();
            assert_eq!(
                rebase_branch(&repo, "refs/heads/main", "side")
                    .await
                    .unwrap(),
                RebaseOutcome::FastForward
            );
        });
        assert_eq!(
            git(&path, ["rev-parse", "side"]),
            git(&path, ["rev-parse", "main"])
        );
        assert_eq!(read(&path, "f.txt"), "two\n");
    }

    #[test]
    fn rebases_onto_a_remote_branch() {
        let fx = Fixture::new();
        let upstream = fx.create_diverged_repo("rebase-upstream", &clean(), None);
        let path = fx.clone_repo("rebase-remote", &upstream);
        commit_files(&path, &files([("c.txt", "clone\n")]), "clone");

        smol::block_on(async {
            let repo = fx.repos.open("rebase-remote").await.unwrap();
            assert_eq!(
                rebase_branch(&repo, "refs/remotes/origin/side", "main")
                    .await
                    .unwrap(),
                RebaseOutcome::Rebased
            );
        });
        assert_eq!(
            git(&path, ["log", "--format=%s", "origin/side..main"]),
            "clone\nours"
        );
        assert_eq!(
            git(&path, ["rev-parse", "main~2"]),
            git(&path, ["rev-parse", "origin/side"])
        );
    }

    #[test]
    fn does_nothing_when_the_branch_is_on_top_of_the_other_already() {
        let fx = Fixture::new();
        let path = fx.create_repo("rebase-up-to-date");
        commit_files(&path, &files([("f.txt", "one\n")]), "first");
        git(&path, ["branch", "old"]);
        commit_files(&path, &files([("f.txt", "two\n")]), "second");
        let before = snapshot(&path);

        smol::block_on(async {
            let repo = fx.repos.open("rebase-up-to-date").await.unwrap();
            assert_eq!(
                rebase_branch(&repo, "refs/heads/old", "main")
                    .await
                    .unwrap(),
                RebaseOutcome::UpToDate
            );
        });
        assert_eq!(snapshot(&path), before);
    }

    #[test]
    fn rebases_onto_the_branch_not_a_tag_of_the_same_name() {
        let fx = Fixture::new();
        let path = create_side_repo(&fx, "rebase-tag-name", &clean(), None);
        git(&path, ["tag", "main", "main~"]);

        smol::block_on(async {
            let repo = fx.repos.open("rebase-tag-name").await.unwrap();
            assert_eq!(
                rebase_branch(&repo, "refs/heads/main", "side")
                    .await
                    .unwrap(),
                RebaseOutcome::Rebased
            );
        });
        assert_eq!(
            git(&path, ["rev-parse", "side~"]),
            git(&path, ["rev-parse", "refs/heads/main"])
        );
    }

    #[test]
    fn keeps_the_untracked_files_that_arent_in_the_way() {
        let fx = Fixture::new();
        let path = create_side_repo(&fx, "rebase-keeps-untracked", &clean(), None);
        fs::write(path.join("new.txt"), "new\n").unwrap();

        smol::block_on(async {
            let repo = fx.repos.open("rebase-keeps-untracked").await.unwrap();
            assert_eq!(
                rebase_branch(&repo, "refs/heads/main", "side")
                    .await
                    .unwrap(),
                RebaseOutcome::Rebased
            );
        });
        assert_eq!(git(&path, ["status", "--porcelain"]), "?? new.txt");
    }

    #[test]
    fn leaves_fixup_commits_as_they_are_whatever_rebase_autosquash_says() {
        let fx = Fixture::new();
        let path = create_side_repo(&fx, "rebase-autosquash", &clean(), None);
        commit_files(&path, &files([("s.txt", "side again\n")]), "fixup! theirs");
        git(&path, ["config", "rebase.autoSquash", "true"]);

        smol::block_on(async {
            let repo = fx.repos.open("rebase-autosquash").await.unwrap();
            assert_eq!(
                rebase_branch(&repo, "refs/heads/main", "side")
                    .await
                    .unwrap(),
                RebaseOutcome::Rebased
            );
        });
        assert_eq!(
            git(&path, ["log", "--format=%s", "main..side"]),
            "fixup! theirs\ntheirs"
        );
    }

    // At conflicts.

    #[test]
    fn stops_leaving_the_rebase_under_way_to_resolve() {
        let fx = Fixture::new();
        let path = create_side_repo(&fx, "rebase-conflicts", &text_conflict(), None);
        smol::block_on(async {
            let repo = fx.repos.open("rebase-conflicts").await.unwrap();

            assert_eq!(
                rebase_branch(&repo, "refs/heads/main", "side")
                    .await
                    .unwrap(),
                RebaseOutcome::Conflicts
            );
            assert_eq!(
                get_operation(&repo).await.unwrap(),
                rebasing_side_onto_main(1, 1)
            );
        });
        let text = read(&path, "f.txt");
        assert!(
            regex::Regex::new(
                r"^one\n<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> [0-9a-f]+ \(theirs\)\nthree\n$"
            )
            .unwrap()
            .is_match(&text),
            "{text}"
        );
        assert_eq!(git(&path, ["status", "--porcelain"]), "UU f.txt");
    }

    #[test]
    fn goes_on_with_the_next_commit_once_theyre_resolved_and_stops_at_its_conflicts_too() {
        let fx = Fixture::new();
        let path = create_side_repo(
            &fx,
            "rebase-resolved",
            &text_conflict(),
            Some("one\ntheirs again\nthree\n"),
        );
        let main = git(&path, ["rev-parse", "main"]);
        smol::block_on(async {
            let repo = fx.repos.open("rebase-resolved").await.unwrap();

            assert_eq!(
                rebase_branch(&repo, "refs/heads/main", "side")
                    .await
                    .unwrap(),
                RebaseOutcome::Conflicts
            );
            assert!(at_step(&get_operation(&repo).await.unwrap(), 1, 2));
            resolve_text(&repo, "one\nboth\nthree\n").await;
            continue_operation(&repo, OperationKind::Rebase)
                .await
                .unwrap();
            // The second commit conflicts with how the first was resolved.
            assert!(at_step(&get_operation(&repo).await.unwrap(), 2, 2));
            assert_eq!(git(&path, ["status", "--porcelain"]), "UU f.txt");

            resolve_text(&repo, "one\nboth again\nthree\n").await;
            continue_operation(&repo, OperationKind::Rebase)
                .await
                .unwrap();
            assert_eq!(get_operation(&repo).await.unwrap(), None);
        });
        assert_eq!(
            git(&path, ["log", "--format=%s", "main..side"]),
            "theirs again\ntheirs"
        );
        assert_eq!(git(&path, ["rev-parse", "side~2"]), main);
        assert_eq!(git(&path, ["show", "HEAD:f.txt"]), "one\nboth again\nthree");
        assert_eq!(git(&path, ["branch", "--show-current"]), "side");
    }

    #[test]
    fn stops_at_conflicts_with_a_remote_branch_named_as_the_user_knows_it() {
        let fx = Fixture::new();
        let upstream = fx.create_diverged_repo("rebase-conflicts-upstream", &text_conflict(), None);
        let path = fx.clone_repo("rebase-conflicts-remote", &upstream);
        git(&path, ["checkout", "-q", "side"]);
        smol::block_on(async {
            let repo = fx.repos.open("rebase-conflicts-remote").await.unwrap();

            assert_eq!(
                rebase_branch(&repo, "refs/remotes/origin/main", "side")
                    .await
                    .unwrap(),
                RebaseOutcome::Conflicts
            );
            assert_eq!(
                get_operation(&repo).await.unwrap(),
                rebasing_side_onto_main(1, 1)
            );
            resolve_text(&repo, "one\nboth\nthree\n").await;
            continue_operation(&repo, OperationKind::Rebase)
                .await
                .unwrap();
        });
        assert_eq!(
            git(&path, ["rev-parse", "side~"]),
            git(&path, ["rev-parse", "origin/main"])
        );
    }

    #[test]
    fn is_aborted_putting_everything_back_as_it_was() {
        let fx = Fixture::new();
        let path = create_side_repo(&fx, "rebase-abort", &text_conflict(), None);
        let before = snapshot(&path);
        smol::block_on(async {
            let repo = fx.repos.open("rebase-abort").await.unwrap();

            rebase_branch(&repo, "refs/heads/main", "side")
                .await
                .unwrap();
            abort_operation(&repo, OperationKind::Rebase).await.unwrap();
        });
        assert_eq!(snapshot(&path), before);
        assert_eq!(read(&path, "f.txt"), "one\ntheirs\nthree\n");
    }

    #[test]
    fn stops_at_a_file_deleted_on_one_side_resolved_by_keeping_a_side_whole() {
        let fx = Fixture::new();
        let sides = Sides {
            base: files([("f.txt", "one\n")]),
            ours: files([("f.txt", "changed\n")]),
            theirs: Files::new().without("f.txt"),
        };
        let path = create_side_repo(&fx, "rebase-deleted", &sides, None);
        smol::block_on(async {
            let repo = fx.repos.open("rebase-deleted").await.unwrap();

            assert_eq!(
                rebase_branch(&repo, "refs/heads/main", "side")
                    .await
                    .unwrap(),
                RebaseOutcome::Conflicts
            );
            let conflict = get_conflict(&repo, "f.txt").await.unwrap();
            // In a rebase, "ours" is the branch rebased onto, and "theirs" the commit replayed.
            assert!(conflict.sides.ours.is_some());
            assert_eq!(conflict.sides.theirs, None);
            let shown = ShownConflict {
                sides: conflict.sides.clone(),
                version: None,
            };
            keep_side(&repo, "f.txt", Side::Ours, &shown).await.unwrap();
            continue_operation(&repo, OperationKind::Rebase)
                .await
                .unwrap();
            assert_eq!(get_operation(&repo).await.unwrap(), None);
        });
        assert_eq!(git(&path, ["show", "HEAD:f.txt"]), "changed");
    }

    // Refuses, doing nothing,

    #[test]
    fn refuses_over_uncommitted_changes_naming_the_files() {
        let fx = Fixture::new();
        let path = create_side_repo(&fx, "rebase-over-changes", &clean(), None);
        fs::write(path.join("f.txt"), "mine\n").unwrap();
        let before = snapshot(&path);

        smol::block_on(async {
            let repo = fx.repos.open("rebase-over-changes").await.unwrap();
            let error = rebase_branch(&repo, "refs/heads/main", "side")
                .await
                .unwrap_err();
            assert_blocked(
                &error,
                "Git won't rebase while there are uncommitted changes to f.txt. Commit or stash them, then rebase.",
            );
        });
        assert_eq!(snapshot(&path), before);
    }

    #[test]
    fn refuses_over_staged_changes() {
        let fx = Fixture::new();
        let sides = Sides {
            base: files([("a b.txt", "a\n"), ("f.txt", "f\n")]),
            ..clean()
        };
        let path = create_side_repo(&fx, "rebase-over-staged", &sides, None);
        fs::write(path.join("a b.txt"), "staged\n").unwrap();
        fs::write(path.join("f.txt"), "staged\n").unwrap();
        git(&path, ["add", "-A"]);
        let before = snapshot(&path);

        smol::block_on(async {
            let repo = fx.repos.open("rebase-over-staged").await.unwrap();
            let error = rebase_branch(&repo, "refs/heads/main", "side")
                .await
                .unwrap_err();
            assert!(
                error.to_string().contains(
                    "Git won't rebase while changes are staged, to a b.txt and f.txt. Commit or stash them, then rebase."
                ),
                "{error}"
            );
        });
        assert_eq!(snapshot(&path), before);
    }

    #[test]
    fn refuses_over_an_untracked_file_in_the_way_of_the_other_branchs() {
        let fx = Fixture::new();
        let path = create_side_repo(&fx, "rebase-over-untracked", &clean(), None);
        fs::write(path.join("m.txt"), "mine\n").unwrap();
        let before = snapshot(&path);

        smol::block_on(async {
            let repo = fx.repos.open("rebase-over-untracked").await.unwrap();
            let error = rebase_branch(&repo, "refs/heads/main", "side")
                .await
                .unwrap_err();
            assert_blocked(
                &error,
                "Rebasing onto main would overwrite the untracked file m.txt. Move or remove it, then rebase.",
            );
        });
        assert_eq!(snapshot(&path), before);
        assert_eq!(read(&path, "m.txt"), "mine\n");
    }

    #[test]
    fn refuses_while_a_merge_is_under_way() {
        let fx = Fixture::new();
        let path = fx.create_diverged_repo("rebase-during-merge", &text_conflict(), None);
        git(&path, ["branch", "other", "main~"]);
        try_git(&path, ["merge", "-q", "side"]);

        smol::block_on(async {
            let repo = fx.repos.open("rebase-during-merge").await.unwrap();
            let error = rebase_branch(&repo, "refs/heads/other", "main")
                .await
                .unwrap_err();
            assert_blocked(
                &error,
                "A merge is under way. Finish or abort it, then rebase.",
            );
        });
    }

    #[test]
    fn refuses_while_a_rebase_is_under_way() {
        let fx = Fixture::new();
        let path = create_side_repo(&fx, "rebase-during-rebase", &text_conflict(), None);
        git(&path, ["branch", "other", "main"]);
        smol::block_on(async {
            let repo = fx.repos.open("rebase-during-rebase").await.unwrap();
            rebase_branch(&repo, "refs/heads/main", "side")
                .await
                .unwrap();
            // Resolved, but not continued yet: still under way.
            resolve_text(&repo, "one\nboth\nthree\n").await;

            let error = rebase_branch(&repo, "refs/heads/other", "side")
                .await
                .unwrap_err();
            assert_blocked(
                &error,
                "A rebase is under way. Finish or abort it, then rebase.",
            );
            assert!(rebasing_side(&get_operation(&repo).await.unwrap()));
        });
    }

    #[test]
    fn refuses_while_a_popped_stashs_conflicts_are_left_to_resolve() {
        let fx = Fixture::new();
        let path = create_side_repo(&fx, "rebase-during-pop", &clean(), None);
        fs::write(path.join("f.txt"), "stashed\n").unwrap();
        git(&path, ["stash", "push", "-q"]);
        commit_files(&path, &files([("f.txt", "committed\n")]), "second");
        smol::block_on(async {
            let repo = fx.repos.open("rebase-during-pop").await.unwrap();
            let stash = git(&path, ["rev-parse", "refs/stash"]);
            let popped = pop_stash(&repo, &stash).await;
            assert!(matches!(popped, Err(Error::StashConflict(_))), "{popped:?}");
            let before = snapshot(&path);

            let error = rebase_branch(&repo, "refs/heads/main", "side")
                .await
                .unwrap_err();
            assert_blocked(
                &error,
                "Some files have conflicts. Resolve them, then rebase.",
            );
            assert_eq!(snapshot(&path), before);
        });
    }

    #[test]
    fn refuses_with_head_detached() {
        let fx = Fixture::new();
        let path = create_side_repo(&fx, "rebase-detached", &clean(), None);
        git(&path, ["checkout", "-q", "--detach"]);
        let before = snapshot(&path);

        smol::block_on(async {
            let repo = fx.repos.open("rebase-detached").await.unwrap();
            let error = rebase_branch(&repo, "refs/heads/main", "side")
                .await
                .unwrap_err();
            assert_blocked(&error, "HEAD is detached: check out a branch to rebase it.");
        });
        assert_eq!(snapshot(&path), before);
    }

    #[test]
    fn refuses_on_a_branch_without_commits_yet() {
        let fx = Fixture::new();
        let path = fx.create_repo("rebase-unborn");
        commit_files(&path, &files([("f.txt", "one\n")]), "first");
        git(&path, ["checkout", "-q", "--orphan", "empty"]);
        git(&path, ["rm", "-rqf", "."]);

        smol::block_on(async {
            let repo = fx.repos.open("rebase-unborn").await.unwrap();
            let error = rebase_branch(&repo, "refs/heads/main", "empty")
                .await
                .unwrap_err();
            assert_blocked(
                &error,
                "empty has no commits yet, so there's nothing to rebase. Merge main into it instead.",
            );
        });
        assert_eq!(git(&path, ["branch", "--show-current"]), "empty");
    }

    #[test]
    fn refuses_once_another_branch_is_checked_out_than_the_one_the_user_saw() {
        let fx = Fixture::new();
        let path = create_side_repo(&fx, "rebase-switched", &clean(), None);
        git(&path, ["checkout", "-qb", "topic"]);
        let before = snapshot(&path);

        smol::block_on(async {
            let repo = fx.repos.open("rebase-switched").await.unwrap();
            let error = rebase_branch(&repo, "refs/heads/main", "side")
                .await
                .unwrap_err();
            assert!(matches!(error, Error::RepositoryChanged(_)), "{error:?}");
            assert_eq!(
                error.to_string(),
                "Switched from side to topic meanwhile, so nothing was rebased."
            );
        });
        assert_eq!(snapshot(&path), before);
    }

    #[test]
    fn refuses_once_the_branch_is_gone() {
        let fx = Fixture::new();
        let path = create_side_repo(&fx, "rebase-gone", &clean(), None);
        git(&path, ["branch", "other", "main"]);
        git(&path, ["branch", "-D", "other"]);

        smol::block_on(async {
            let repo = fx.repos.open("rebase-gone").await.unwrap();
            let error = rebase_branch(&repo, "refs/heads/other", "side")
                .await
                .unwrap_err();
            assert_blocked(&error, "other no longer exists.");
        });
    }

    #[test]
    fn refuses_onto_itself() {
        let fx = Fixture::new();
        let path = create_side_repo(&fx, "rebase-itself", &clean(), None);
        let before = snapshot(&path);

        smol::block_on(async {
            let repo = fx.repos.open("rebase-itself").await.unwrap();
            let error = rebase_branch(&repo, "refs/heads/side", "side")
                .await
                .unwrap_err();
            assert_blocked(&error, "Can't rebase side onto itself.");
        });
        assert_eq!(snapshot(&path), before);
    }

    #[test]
    fn refuses_onto_what_isnt_a_branch() {
        let fx = Fixture::new();
        create_side_repo(&fx, "rebase-onto-tag", &clean(), None);
        smol::block_on(async {
            let repo = fx.repos.open("rebase-onto-tag").await.unwrap();
            // As the UI's input is checked, as is a missing branch to rebase.
            for (reference, branch, message) in [
                ("refs/tags/v1", "side", "refs/tags/v1 isn't a branch."),
                ("refs/heads/main", "", "The branch name is empty."),
            ] {
                let error = rebase_branch(&repo, reference, branch).await.unwrap_err();
                assert!(matches!(error, Error::InvalidInput(_)), "{error:?}");
                assert_eq!(error.to_string(), message);
            }
        });
    }

    #[test]
    fn is_undone_when_a_commit_cant_be_signed_saying_why() {
        let fx = Fixture::new();
        let path = create_side_repo(&fx, "rebase-unsigned", &clean(), None);
        let before = snapshot(&path);
        let gpg = path.join(".git").join("no-gpg");
        write_script(&gpg, "#!/bin/sh\nexit 1\n");
        git(&path, ["config", "commit.gpgSign", "true"]);
        // With gpg, whatever the user's global config signs with (ssh, say): `gpg.program` is gpg's.
        git(&path, ["config", "gpg.format", "openpgp"]);
        git(&path, ["config", "gpg.program", gpg.to_str().unwrap()]);
        smol::block_on(async {
            let repo = fx.repos.open("rebase-unsigned").await.unwrap();

            let error = rebase_branch(&repo, "refs/heads/main", "side")
                .await
                .unwrap_err();
            assert!(matches!(error, Error::RebaseUndone { .. }), "{error:?}");
            assert!(
                regex::Regex::new(
                    r"(?s)^Rebasing side onto main stopped partway, and was undone:\n.*failed to write commit object\nFix that, then rebase again\.$"
                )
                .unwrap()
                .is_match(&error.to_string()),
                "{error}"
            );
            assert_eq!(get_operation(&repo).await.unwrap(), None);
            assert_eq!(snapshot(&path), before);

            git(&path, ["config", "commit.gpgSign", "false"]);
            assert_eq!(
                rebase_branch(&repo, "refs/heads/main", "side")
                    .await
                    .unwrap(),
                RebaseOutcome::Rebased
            );
        });
        assert_eq!(git(&path, ["log", "--format=%s", "main..side"]), "theirs");
    }

    #[test]
    fn is_left_under_way_when_it_stops_partway_and_cant_be_undone_either() {
        let fx = Fixture::new();
        let path = create_side_repo(&fx, "rebase-stuck", &clean(), None);
        // Fails to sign, and takes away what aborting the rebase needs, as a crash might.
        let gpg = path.join(".git").join("no-gpg");
        let orig_head = path.join(".git").join("rebase-merge").join("orig-head");
        write_script(
            &gpg,
            &format!(
                "#!/bin/sh\nrm -f \"{}\"\nexit 1\n",
                orig_head.to_str().unwrap()
            ),
        );
        git(&path, ["config", "commit.gpgSign", "true"]);
        git(&path, ["config", "gpg.format", "openpgp"]);
        git(&path, ["config", "gpg.program", gpg.to_str().unwrap()]);
        smol::block_on(async {
            let repo = fx.repos.open("rebase-stuck").await.unwrap();

            let error = rebase_branch(&repo, "refs/heads/main", "side")
                .await
                .unwrap_err();
            assert!(matches!(error, Error::RebaseStopped { .. }), "{error:?}");
            assert!(
                regex::Regex::new(
                    r"(?s)^Rebasing side onto main stopped partway:\n.*failed to write commit object\nIt couldn't be undone either:\n.+\nFinish the rebase in a terminal, or abort it\.$"
                )
                .unwrap()
                .is_match(&error.to_string()),
                "{error}"
            );
            assert!(rebasing_side(&get_operation(&repo).await.unwrap()));
        });
    }

    #[test]
    fn moves_the_branch_to_the_other_when_the_other_has_its_commits_already() {
        let fx = Fixture::new();
        let path = create_side_repo(&fx, "rebase-upstream-already", &clean(), None);
        git(&path, ["checkout", "-q", "main"]);
        git(&path, ["cherry-pick", "side"]);
        git(&path, ["checkout", "-q", "side"]);

        smol::block_on(async {
            let repo = fx.repos.open("rebase-upstream-already").await.unwrap();
            assert_eq!(
                rebase_branch(&repo, "refs/heads/main", "side")
                    .await
                    .unwrap(),
                RebaseOutcome::FastForward
            );
        });
        assert_eq!(
            git(&path, ["rev-parse", "side"]),
            git(&path, ["rev-parse", "main"])
        );
    }

    #[test]
    fn says_what_git_said_when_it_failed_otherwise() {
        let fx = Fixture::new();
        let path = create_side_repo(&fx, "rebase-otherwise", &clean(), None);
        git(&path, ["config", "rebase.backend", "no-such-backend"]);

        smol::block_on(async {
            let repo = fx.repos.open("rebase-otherwise").await.unwrap();
            let error = rebase_branch(&repo, "refs/heads/main", "side")
                .await
                .unwrap_err();
            assert!(matches!(error, Error::Git(_)), "{error:?}");
            assert!(error.to_string().contains("no-such-backend"), "{error}");
        });
    }

    // What git says when it refuses to rebase over uncommitted changes.

    #[test]
    fn is_read_for_unstaged_and_staged_ones() {
        assert_eq!(
            parse_uncommitted(
                "error: cannot rebase: You have unstaged changes.\nerror: Please commit or stash them.\n"
            ),
            Some(Uncommitted::Unstaged)
        );
        assert_eq!(
            parse_uncommitted(
                "error: cannot rebase: Your index contains uncommitted changes.\nerror: Please commit or stash them.\n"
            ),
            Some(Uncommitted::Staged)
        );
        assert_eq!(parse_uncommitted("fatal: invalid upstream 'nope'\n"), None);
    }
}
