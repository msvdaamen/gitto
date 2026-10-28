//! Reading the operation under way, and continuing or aborting it. Ported from
//! `packages/git/src/features/operation/commands.ts`.

use std::path::{Path, PathBuf};

use super::schema::{CommitName, Operation, OperationAction, OperationKind, Steps};
use crate::core::errors::{Error, Result};
use crate::core::git_dirs::git_dirs;
use crate::core::repo::{GitCommand, Repo, current_branch, has_conflicts, resolve_ref, succeeds};
use crate::core::runner::RunOptions;

/// The contents of `file`, trimmed; `None` if it isn't there, or has nothing in it.
async fn read_state(file: PathBuf) -> Option<String> {
    let bytes = async_fs::read(file).await.ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// Whether `file` is there; a folder counts.
async fn file_exists(file: PathBuf) -> bool {
    async_fs::metadata(file).await.is_ok()
}

/// Whether a rebase, or `git am`, is stopped in a worktree (see `rebase_state`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RebaseState {
    pub rebasing: bool,
    pub applying: bool,
}

/// Whether a rebase, or `git am`, is stopped in the worktree whose git directory is `git_dir`:
/// they keep their state there meanwhile, `am` where some rebases do, with a file saying it's
/// `am`'s.
pub async fn rebase_state(git_dir: &Path) -> RebaseState {
    let (rebase_merge, rebase_apply, applying) = futures::join!(
        file_exists(git_dir.join("rebase-merge")),
        file_exists(git_dir.join("rebase-apply")),
        file_exists(git_dir.join("rebase-apply").join("applying")),
    );
    RebaseState {
        rebasing: rebase_merge || (rebase_apply && !applying),
        applying,
    }
}

/// What `operation_state` reads: the operation under way, and the files it's told by.
#[derive(Debug, Clone, Default)]
struct OperationState {
    kind: Option<OperationKind>,
    /// MERGE_HEAD, CHERRY_PICK_HEAD and REVERT_HEAD, if they're there.
    merge: Option<String>,
    cherry_pick: Option<String>,
    revert: Option<String>,
    /// The commands left in a series of cherry-picks or reverts, the one it stopped at first.
    todo: Vec<String>,
}

/// Which operation is under way in the worktree whose git directory is `git_dir`, going by the
/// files git keeps while it is; `None` if none is. A rebase first: one stopped at a merge it
/// replays has a MERGE_HEAD too. A series of cherry-picks or reverts is under way while its todo
/// list is there, also once the commit it stopped at was committed by hand, which takes
/// CHERRY_PICK_HEAD away: the rest are still to be picked.
async fn operation_state(git_dir: &Path) -> OperationState {
    let (state, merge, cherry_pick, revert, todo) = futures::join!(
        rebase_state(git_dir),
        read_state(git_dir.join("MERGE_HEAD")),
        read_state(git_dir.join("CHERRY_PICK_HEAD")),
        read_state(git_dir.join("REVERT_HEAD")),
        read_state(git_dir.join("sequencer").join("todo")),
    );
    let todo = todo_lines(todo.as_deref());
    // The todo list's commands say which a series is: `pick` or `revert`.
    let command = todo.first().and_then(|line| line.split_whitespace().next());
    let kind = if state.applying {
        Some(OperationKind::Am)
    } else if state.rebasing {
        Some(OperationKind::Rebase)
    } else if merge.is_some() {
        Some(OperationKind::Merge)
    } else if cherry_pick.is_some() || matches!(command, Some("pick" | "p")) {
        Some(OperationKind::CherryPick)
    } else if revert.is_some() || command == Some("revert") {
        Some(OperationKind::Revert)
    } else {
        None
    };
    OperationState {
        kind,
        merge,
        cherry_pick,
        revert,
        todo,
    }
}

/// The commands in a todo list (the sequencer's, or a rebase's), its comments and blanks aside.
fn todo_lines(todo: Option<&str>) -> Vec<String> {
    todo.unwrap_or_default()
        .split('\n')
        .filter(|line| {
            line.chars()
                .next()
                .is_some_and(|first| first != '#' && !first.is_whitespace())
        })
        .map(str::to_owned)
        .collect()
}

/// The operation under way in the repository, with what the user's told of it; `None` if none.
pub async fn get_operation(repo: &Repo) -> Result<Option<Operation>> {
    let git_dir = git_dirs(repo).await?.git_dir;
    let state = |name: &str| read_state(git_dir.join(name));
    let run = repo.git();
    let operation = operation_state(&git_dir).await;
    let Some(kind) = operation.kind else {
        return Ok(None);
    };
    Ok(Some(match kind {
        OperationKind::Merge => {
            // An octopus merge has a line for each of them; the first is said.
            let merging = operation
                .merge
                .as_deref()
                .and_then(|merge| merge.split('\n').next())
                .unwrap_or_default();
            let (name, into) = futures::join!(ref_name(&run, merging), current_branch(&run));
            Operation::Merge {
                merging: name,
                into: into?,
            }
        }
        OperationKind::Rebase => {
            // `rebase-merge` for the merge backend, git's default, which has `onto` there;
            // `rebase-apply` for the apply one.
            let (merging, onto) = match state("rebase-merge/onto").await {
                Some(onto) => (true, Some(onto)),
                None => (false, state("rebase-apply/onto").await),
            };
            let dir = if merging {
                "rebase-merge"
            } else {
                "rebase-apply"
            };
            let onto_name = async {
                match &onto {
                    Some(onto) => Some(ref_name(&run, onto).await),
                    None => None,
                }
            };
            let (head_name, onto, step, total) = futures::join!(
                state(&format!("{dir}/head-name")),
                onto_name,
                state(&format!(
                    "{dir}/{}",
                    if merging { "msgnum" } else { "next" }
                )),
                state(&format!("{dir}/{}", if merging { "end" } else { "last" })),
            );
            Operation::Rebase {
                branch: head_name
                    .as_deref()
                    .and_then(|name| name.strip_prefix("refs/heads/"))
                    .map(str::to_owned),
                onto,
                steps: steps(step.as_deref(), total.as_deref()),
            }
        }
        OperationKind::CherryPick | OperationKind::Revert => {
            let head = operation.cherry_pick.or(operation.revert);
            // None between the commits of a series, once the one it stopped at was committed by
            // hand.
            let commit = match head {
                Some(head) => Some(commit_name(&run, &head).await),
                None => None,
            };
            // A line for the one it stopped at and each one after it.
            let remaining = operation.todo.len().saturating_sub(1) as u64;
            if kind == OperationKind::CherryPick {
                Operation::CherryPick { commit, remaining }
            } else {
                Operation::Revert { commit, remaining }
            }
        }
        OperationKind::Am => {
            let (step, total) =
                futures::join!(state("rebase-apply/next"), state("rebase-apply/last"));
            Operation::Am {
                steps: steps(step.as_deref(), total.as_deref()),
            }
        }
    }))
}

/// How far an operation has got, from git's numbers for it; `None` if they don't make sense. Git
/// writes them as plain decimal counters.
fn steps(step: Option<&str>, total: Option<&str>) -> Option<Steps> {
    let step = step?.parse::<u64>().ok()?;
    let total = total?.parse::<u64>().ok()?;
    (step > 0 && total >= step).then_some(Steps { step, total })
}

/// The shortest name of the commit `sha`, as the user knows it: a branch pointing at it, or else a
/// remote-tracking branch or a tag, or else its abbreviated SHA; or `sha` itself, if git can't
/// say.
async fn ref_name(run: &GitCommand, sha: &str) -> String {
    let points_at = format!("--points-at={sha}");
    let refs = run
        .run([
            "for-each-ref",
            &points_at,
            "--format=%(refname)",
            "refs/heads",
            "refs/remotes",
            "refs/tags",
        ])
        .await
        .unwrap_or_default();
    let names: Vec<&str> = refs.split('\n').filter(|name| !name.is_empty()).collect();
    for prefix in ["refs/heads/", "refs/remotes/", "refs/tags/"] {
        if let Some(name) = names.iter().find_map(|name| name.strip_prefix(prefix)) {
            return name.to_owned();
        }
    }
    run.run(["rev-parse", "--short", sha])
        .await
        .unwrap_or_else(|_| sha.to_owned())
        .trim()
        .to_owned()
}

/// The commit `sha`'s abbreviated SHA and subject.
async fn commit_name(run: &GitCommand, sha: &str) -> CommitName {
    let output = run
        .run(["log", "-1", "--format=%h%x00%s", sha, "--"])
        .await
        .unwrap_or_default();
    let mut parts = output.trim().split('\0');
    let short = parts
        .next()
        .map_or_else(|| sha.chars().take(7).collect(), str::to_owned);
    let subject = parts.next().unwrap_or_default().to_owned();
    CommitName {
        sha: short,
        subject,
    }
}

/// The command that continues or aborts `kind`: `git merge --continue`, `git rebase --abort`, and
/// so on.
fn operation_args(kind: OperationKind, action: OperationAction) -> [&'static str; 2] {
    [kind.as_str(), action.as_flag()]
}

/// The operation under way in the worktree whose git directory is `git_dir`, if any (see
/// `operation_state`).
pub async fn operation_under_way(git_dir: &Path) -> Option<OperationKind> {
    operation_state(git_dir).await.kind
}

/// Checks that the operation under way is `kind`, the one the user saw; fails with
/// `Error::NoOperation` if it isn't, e.g. as it was finished in a terminal meanwhile.
async fn check_operation(git_dir: &Path, kind: OperationKind) -> Result<()> {
    let current = operation_state(git_dir).await.kind;
    if current == Some(kind) {
        return Ok(());
    }
    Err(Error::NoOperation {
        expected: kind,
        current,
    })
}

/// Continues the operation `kind` once its conflicts are resolved: commits the merge, or the commit
/// cherry-picked, reverted or replayed, with the message git prepared, and goes on with the next.
/// One that stops at the next one's conflicts is left to resolve them too, as git does; that isn't
/// a failure. A cherry-pick or revert whose conflicts were resolved to nothing, which git won't
/// commit, is skipped, as git suggests: there's nothing of it to keep. Only that one: a later one
/// that turns out empty is left for the user, as git leaves it.
///
/// Fails with `Error::EditorNeeded` for a rebase that rewords or squashes commits from here on:
/// their messages are the user's to write, in an editor, which Gitto can't open.
pub async fn continue_operation(repo: &Repo, kind: OperationKind) -> Result<()> {
    repo.exclusive(|run| async move {
        let git_dir = git_dirs(repo).await?.git_dir;
        let (conflicted, before, ()) = futures::try_join!(
            has_conflicts(&run),
            stopped_at(&run, &git_dir),
            check_operation(&git_dir, kind),
        )?;
        if kind == OperationKind::Rebase && rewords_or_squashes(&git_dir).await {
            return Err(Error::EditorNeeded);
        }
        // With the message as git prepared it, as no editor can be opened: `:` takes it as it is,
        // and leaves out its comments, like the conflicts git lists in a merge's.
        let options = RunOptions::default()
            .env("GIT_EDITOR", ":")
            .rewrites_files();
        let continued = go_on(
            &run,
            kind,
            operation_args(kind, OperationAction::Continue),
            conflicted,
            &options,
        )
        .await;
        let Err(error) = continued else {
            return Ok(());
        };
        if error.as_git().is_none() {
            return Err(error);
        }
        // Still at the commit it stopped at, with nothing staged: resolved to nothing.
        let empty = matches!(kind, OperationKind::CherryPick | OperationKind::Revert)
            && !conflicted
            && before.is_some()
            && stopped_at(&run, &git_dir).await? == before
            && succeeds(&run, ["diff", "--cached", "--quiet"]).await?;
        if !empty {
            return Err(error.without_hints());
        }
        go_on(
            &run,
            kind,
            operation_args(kind, OperationAction::Skip),
            false,
            &options,
        )
        .await
        .map_err(Error::without_hints)
    })
    .await
}

/// Runs `args`, which go on with the operation `kind`; succeeds if they stop at new conflicts.
async fn go_on(
    run: &GitCommand,
    kind: OperationKind,
    args: [&'static str; 2],
    conflicted_before: bool,
    options: &RunOptions,
) -> Result<()> {
    let Err(error) = run.run_with(args, options.clone()).await else {
        return Ok(());
    };
    // Not if they were there before: git refused to go on with them, and says so.
    if error.as_git().is_some()
        && !conflicted_before
        && kind != OperationKind::Merge
        && has_conflicts(run).await?
    {
        return Ok(());
    }
    Err(error)
}

/// Where a cherry-pick or revert stopped: HEAD, and the commit it stopped at; `None` if it isn't
/// stopped at one.
async fn stopped_at(run: &GitCommand, git_dir: &Path) -> Result<Option<(Option<String>, String)>> {
    let (head, cherry_pick, revert) = futures::join!(
        resolve_ref(run, "HEAD"),
        read_state(git_dir.join("CHERRY_PICK_HEAD")),
        read_state(git_dir.join("REVERT_HEAD")),
    );
    let head = head?;
    Ok(cherry_pick.or(revert).map(|at| (head, at)))
}

/// Whether the rebase stopped in the worktree whose git directory is `git_dir` asks for a commit
/// message in an editor from here on: for a commit it rewords or squashes, the one it stopped at
/// included, or one a squash it's in the middle of ends with, the squash itself done already.
async fn rewords_or_squashes(git_dir: &Path) -> bool {
    let (todo, done) = futures::join!(
        read_state(git_dir.join("rebase-merge").join("git-rebase-todo")),
        read_state(git_dir.join("rebase-merge").join("done")),
    );
    let done_lines = todo_lines(done.as_deref());
    // The one it stopped at, and the squashes and fixups that run up to it, if it's one of them.
    let mut from = done_lines.len().saturating_sub(1);
    while from > 0 && squashing(&done_lines[from]) {
        from -= 1;
    }
    done_lines[from..]
        .iter()
        .chain(&todo_lines(todo.as_deref()))
        .any(|line| asks_for_message(line))
}

/// A line of a rebase's todo list: its command, and the word after it (a commit, or a flag).
fn todo_command(line: &str) -> (Option<&str>, Option<&str>) {
    let mut words = line.split_whitespace();
    (words.next(), words.next())
}

/// Whether a line of a rebase's todo list is a squash, or a fixup, which a squash before it in a
/// row of them is asked for with.
fn squashing(line: &str) -> bool {
    matches!(
        todo_command(line),
        (Some("s" | "squash" | "f" | "fixup"), Some(_))
    )
}

/// Whether git asks for the message of the commit a line of a rebase's todo list makes, in an
/// editor: a reword, a squash, `fixup -c` (not `-C`, which takes the fixup's message as it is), and
/// a merge but with `-C`, which takes the merge's message as it is.
fn asks_for_message(line: &str) -> bool {
    match todo_command(line) {
        (Some("r" | "reword" | "s" | "squash"), Some(_)) => true,
        (Some("f" | "fixup"), Some(flag)) => flag == "-c",
        (Some("m" | "merge"), Some(flag)) => flag != "-C",
        _ => false,
    }
}

/// Aborts the operation `kind`, putting the repository back as it was before it started: the
/// conflicts and how they were resolved so far go with it.
pub async fn abort_operation(repo: &Repo, kind: OperationKind) -> Result<()> {
    repo.exclusive(|run| async move {
        check_operation(&git_dirs(repo).await?.git_dir, kind).await?;
        run.run_with(
            operation_args(kind, OperationAction::Abort),
            RunOptions::default().rewrites_files(),
        )
        .await
        .map_err(Error::without_hints)?;
        Ok(())
    })
    .await
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process::Command;

    use super::*;
    use crate::test_support::conflicts::{Sides, commit_files, files, text_conflict, try_git};
    use crate::test_support::{Fixture, git};

    /// Resolves `f.txt`'s conflict as `contents`, and marks it resolved.
    ///
    /// TS saved it with `saveWorkingTreeFile` and staged it with `markResolved`, of features not
    /// ported yet; this writes and stages it as they do.
    fn resolve(path: &Path, contents: &str) {
        fs::write(path.join("f.txt"), contents).unwrap();
        git(path, ["add", "--all", "--", "f.txt"]);
    }

    /// `f.txt`'s contents as the conflict has them (TS's `getConflict(...).text.contents`).
    fn conflict_text(path: &Path) -> String {
        fs::read_to_string(path.join("f.txt")).unwrap()
    }

    /// Runs `git rebase -i main` in `path`, its todo list edited by the perl `script`.
    fn rebase_interactively(path: &Path, script: &str) {
        Command::new("git")
            .args(["rebase", "-i", "main"])
            .current_dir(path)
            .env("GIT_SEQUENCE_EDITOR", format!("perl -pi -e '{script}'"))
            .output()
            .expect("git runs");
    }

    // a merge

    #[test]
    fn a_merge_is_under_way_once_it_stops_at_conflicts_and_continues_once_theyre_resolved() {
        let fx = Fixture::new();
        let path = fx.create_merge_conflict("op-merge", None, &text_conflict());
        smol::block_on(async {
            let repo = fx.repos.open("op-merge").await.unwrap();
            assert_eq!(
                get_operation(&repo).await.unwrap(),
                Some(Operation::Merge {
                    merging: "side".into(),
                    into: Some("main".into())
                })
            );

            // Not with conflicts left: git says why.
            let error = continue_operation(&repo, OperationKind::Merge)
                .await
                .unwrap_err();
            assert!(error.as_git().is_some(), "{error:?}");
            resolve(&path, "one\nresolved\nthree\n");
            continue_operation(&repo, OperationKind::Merge)
                .await
                .unwrap();

            assert_eq!(get_operation(&repo).await.unwrap(), None);
            // With the message git prepared, without the conflicts it listed in a comment.
            assert_eq!(
                git(&path, ["log", "-1", "--format=%B"]),
                "Merge branch 'side'"
            );
            assert_eq!(
                git(&path, ["rev-parse", "HEAD^2"]),
                git(&path, ["rev-parse", "side"])
            );
        });
    }

    #[test]
    fn a_merge_is_aborted_which_puts_things_back_as_they_were() {
        let fx = Fixture::new();
        let path = fx.create_merge_conflict("op-merge-abort", None, &text_conflict());
        smol::block_on(async {
            let repo = fx.repos.open("op-merge-abort").await.unwrap();
            abort_operation(&repo, OperationKind::Merge).await.unwrap();

            assert_eq!(get_operation(&repo).await.unwrap(), None);
            assert_eq!(
                fs::read_to_string(path.join("f.txt")).unwrap(),
                "one\nours\nthree\n"
            );
            assert_eq!(git(&path, ["status", "--porcelain"]), "");
        });
    }

    #[test]
    fn a_merge_names_a_commit_no_branch_points_at_by_its_sha() {
        let fx = Fixture::new();
        let path = fx.create_diverged_repo(
            "op-merge-sha",
            &Sides {
                base: files([("f.txt", "one\n")]),
                ours: files([("f.txt", "ours\n")]),
                theirs: files([("f.txt", "theirs\n")]),
            },
            None,
        );
        let side = git(&path, ["rev-parse", "side"]);
        git(&path, ["branch", "-D", "side"]);
        try_git(&path, ["merge", &side]);
        let short = git(&path, ["rev-parse", "--short", &side]).len();
        smol::block_on(async {
            let repo = fx.repos.open("op-merge-sha").await.unwrap();
            assert_eq!(
                get_operation(&repo).await.unwrap(),
                Some(Operation::Merge {
                    merging: side[..short].to_owned(),
                    into: Some("main".into())
                })
            );
        });
    }

    #[test]
    fn a_merge_isnt_continued_or_aborted_as_another_operation() {
        let fx = Fixture::new();
        fx.create_merge_conflict("op-wrong-kind", None, &text_conflict());
        smol::block_on(async {
            let repo = fx.repos.open("op-wrong-kind").await.unwrap();
            let error = abort_operation(&repo, OperationKind::Rebase)
                .await
                .unwrap_err();
            assert!(
                matches!(
                    error,
                    Error::NoOperation {
                        expected: OperationKind::Rebase,
                        current: Some(OperationKind::Merge)
                    }
                ),
                "{error:?}"
            );
            assert_eq!(error.to_string(), "A merge is under way now, not a rebase.");
            abort_operation(&repo, OperationKind::Merge).await.unwrap();
            let error = continue_operation(&repo, OperationKind::Merge)
                .await
                .unwrap_err();
            assert!(
                matches!(
                    error,
                    Error::NoOperation {
                        expected: OperationKind::Merge,
                        current: None
                    }
                ),
                "{error:?}"
            );
            assert_eq!(error.to_string(), "The merge is no longer under way.");
        });
    }

    /// A repository whose `side`, with two commits that change `f.txt`'s second line, is rebased
    /// onto `main`.
    fn create_rebase(fx: &Fixture, name: &str) -> PathBuf {
        let path = fx.create_diverged_repo(name, &text_conflict(), None);
        git(&path, ["checkout", "-q", "side"]);
        commit_files(
            &path,
            &files([("f.txt", "one\ntheirs again\nthree\n")]),
            "theirs again",
        );
        try_git(&path, ["rebase", "main"]);
        path
    }

    // a rebase

    #[test]
    fn a_rebase_says_which_commit_its_at_and_stops_at_the_next_ones_conflicts() {
        let fx = Fixture::new();
        let path = create_rebase(&fx, "op-rebase");
        smol::block_on(async {
            let repo = fx.repos.open("op-rebase").await.unwrap();
            assert_eq!(
                get_operation(&repo).await.unwrap(),
                Some(Operation::Rebase {
                    branch: Some("side".into()),
                    onto: Some("main".into()),
                    steps: Some(Steps { step: 1, total: 2 }),
                })
            );
            let theirs = regex::Regex::new(r"(?m)^>>>>>>> [0-9a-f]+ \(theirs\)$").unwrap();
            let text = conflict_text(&path);
            assert!(theirs.is_match(&text), "{text}");

            // The next commit conflicts with how this one was resolved: that's left to resolve too.
            resolve(&path, "one\nresolved\nthree\n");
            continue_operation(&repo, OperationKind::Rebase)
                .await
                .unwrap();
            let operation = get_operation(&repo).await.unwrap();
            assert!(
                matches!(
                    operation,
                    Some(Operation::Rebase {
                        steps: Some(Steps { step: 2, total: 2 }),
                        ..
                    })
                ),
                "{operation:?}"
            );

            resolve(&path, "one\nresolved again\nthree\n");
            continue_operation(&repo, OperationKind::Rebase)
                .await
                .unwrap();
            assert_eq!(get_operation(&repo).await.unwrap(), None);
            assert_eq!(
                git(&path, ["log", "--format=%s", "main..side"]),
                "theirs again\ntheirs"
            );
        });
    }

    #[test]
    fn a_rebase_says_it_doesnt_know_what_its_onto_when_gits_record_of_that_cant_be_read() {
        let fx = Fixture::new();
        let path = create_rebase(&fx, "op-rebase-onto");
        fs::remove_file(path.join(".git").join("rebase-merge").join("onto")).unwrap();
        fs::write(path.join(".git").join("rebase-merge").join("onto"), "\n").unwrap();
        smol::block_on(async {
            let repo = fx.repos.open("op-rebase-onto").await.unwrap();
            let operation = get_operation(&repo).await.unwrap();
            assert!(
                matches!(operation, Some(Operation::Rebase { onto: None, .. })),
                "{operation:?}"
            );
        });
    }

    #[test]
    fn a_rebase_isnt_continued_when_a_commit_ahead_is_to_be_reworded_which_needs_an_editor() {
        let fx = Fixture::new();
        let path = fx.create_diverged_repo("op-rebase-reword", &text_conflict(), None);
        git(&path, ["checkout", "-q", "side"]);
        commit_files(&path, &files([("g.txt", "g\n")]), "another");
        // An interactive rebase, its second commit to be reworded.
        rebase_interactively(&path, "s/^pick/reword/ if $. == 2");
        smol::block_on(async {
            let repo = fx.repos.open("op-rebase-reword").await.unwrap();
            resolve(&path, "one\nresolved\nthree\n");

            let error = continue_operation(&repo, OperationKind::Rebase)
                .await
                .unwrap_err();
            assert!(matches!(error, Error::EditorNeeded), "{error:?}");
            let operation = get_operation(&repo).await.unwrap();
            assert!(
                matches!(
                    operation,
                    Some(Operation::Rebase {
                        steps: Some(Steps { step: 1, .. }),
                        ..
                    })
                ),
                "{operation:?}"
            );
        });
    }

    #[test]
    fn a_rebase_isnt_continued_at_a_fixup_that_ends_a_squash_whose_message_needs_an_editor() {
        let fx = Fixture::new();
        let path = fx.create_diverged_repo(
            "op-rebase-squash",
            &Sides {
                base: files([("f.txt", "one\ntwo\nthree\n")]),
                ours: files([("f.txt", "one\nours\nthree\n")]),
                theirs: files([("g.txt", "g\n")]),
            },
            None,
        );
        git(&path, ["checkout", "-q", "side"]);
        commit_files(&path, &files([("h.txt", "h\n")]), "squashed");
        commit_files(
            &path,
            &files([("f.txt", "one\ntheirs\nthree\n")]),
            "fixed up",
        );
        // Picks the first, squashes the second into it, and fixes it up with the third, which
        // conflicts.
        rebase_interactively(
            &path,
            "s/^pick/squash/ if $. == 2; s/^pick/fixup/ if $. == 3",
        );
        smol::block_on(async {
            let repo = fx.repos.open("op-rebase-squash").await.unwrap();
            resolve(&path, "one\nresolved\nthree\n");

            let error = continue_operation(&repo, OperationKind::Rebase)
                .await
                .unwrap_err();
            assert!(matches!(error, Error::EditorNeeded), "{error:?}");
        });
    }

    #[test]
    fn a_rebase_is_aborted_back_on_the_branch_as_it_was() {
        let fx = Fixture::new();
        let path = create_rebase(&fx, "op-rebase-abort");
        let before = git(&path, ["rev-parse", "ORIG_HEAD"]);
        smol::block_on(async {
            let repo = fx.repos.open("op-rebase-abort").await.unwrap();
            abort_operation(&repo, OperationKind::Rebase).await.unwrap();

            assert_eq!(get_operation(&repo).await.unwrap(), None);
            assert_eq!(git(&path, ["rev-parse", "HEAD"]), before);
            assert_eq!(git(&path, ["branch", "--show-current"]), "side");
        });
    }

    // a cherry-pick

    #[test]
    fn a_cherry_pick_names_the_commit_and_commits_it_once_resolved() {
        let fx = Fixture::new();
        let path = fx.create_diverged_repo("op-cherry-pick", &text_conflict(), None);
        try_git(&path, ["cherry-pick", "side"]);
        smol::block_on(async {
            let repo = fx.repos.open("op-cherry-pick").await.unwrap();
            assert_eq!(
                get_operation(&repo).await.unwrap(),
                Some(Operation::CherryPick {
                    commit: Some(CommitName {
                        sha: git(&path, ["rev-parse", "--short", "side"]),
                        subject: "theirs".into(),
                    }),
                    remaining: 0,
                })
            );

            resolve(&path, "one\nours and theirs\nthree\n");
            continue_operation(&repo, OperationKind::CherryPick)
                .await
                .unwrap();
            assert_eq!(get_operation(&repo).await.unwrap(), None);
            assert_eq!(git(&path, ["log", "-1", "--format=%s"]), "theirs");
        });
    }

    #[test]
    fn a_cherry_pick_skips_a_commit_whose_conflicts_were_resolved_to_nothing_which_git_wont_commit()
    {
        let fx = Fixture::new();
        let path = fx.create_diverged_repo("op-cherry-pick-empty", &text_conflict(), None);
        let head = git(&path, ["rev-parse", "HEAD"]);
        try_git(&path, ["cherry-pick", "side"]);
        smol::block_on(async {
            let repo = fx.repos.open("op-cherry-pick-empty").await.unwrap();

            resolve(&path, "one\nours\nthree\n");
            continue_operation(&repo, OperationKind::CherryPick)
                .await
                .unwrap();
            assert_eq!(get_operation(&repo).await.unwrap(), None);
            assert_eq!(git(&path, ["rev-parse", "HEAD"]), head);
        });
    }

    #[test]
    fn a_cherry_pick_skips_only_the_commit_it_stopped_at_not_a_later_one_that_turns_out_empty() {
        let fx = Fixture::new();
        let path = fx.create_diverged_repo(
            "op-cherry-pick-later-empty",
            &Sides {
                base: files([("f.txt", "one\ntwo\nthree\n")]),
                ours: files([("f.txt", "one\nours\nthree\n"), ("g.txt", "g\n")]),
                theirs: files([("f.txt", "one\ntheirs\nthree\n")]),
            },
            None,
        );
        // Already on main: empty once it's picked.
        git(&path, ["checkout", "-q", "side"]);
        commit_files(&path, &files([("g.txt", "g\n")]), "add g");
        git(&path, ["checkout", "-q", "main"]);
        try_git(&path, ["cherry-pick", "main..side"]);
        smol::block_on(async {
            let repo = fx.repos.open("op-cherry-pick-later-empty").await.unwrap();

            resolve(&path, "one\nours and theirs\nthree\n");
            let error = continue_operation(&repo, OperationKind::CherryPick)
                .await
                .unwrap_err();
            assert!(error.as_git().is_some(), "{error:?}");
            assert_eq!(git(&path, ["log", "-1", "--format=%s"]), "theirs");
            let operation = get_operation(&repo).await.unwrap();
            assert!(
                matches!(
                    &operation,
                    Some(Operation::CherryPick {
                        commit: Some(CommitName { subject, .. }),
                        ..
                    }) if subject == "add g"
                ),
                "{operation:?}"
            );
        });
    }

    #[test]
    fn a_cherry_pick_is_still_under_way_once_the_commit_it_stopped_at_was_committed_by_hand() {
        let fx = Fixture::new();
        let path = fx.create_diverged_repo("op-cherry-picks-by-hand", &text_conflict(), None);
        git(&path, ["checkout", "-q", "side"]);
        commit_files(&path, &files([("g.txt", "g\n")]), "another");
        git(&path, ["checkout", "-q", "main"]);
        try_git(&path, ["cherry-pick", "main..side"]);
        smol::block_on(async {
            let repo = fx.repos.open("op-cherry-picks-by-hand").await.unwrap();
            resolve(&path, "one\nresolved\nthree\n");
            git(&path, ["commit", "-q", "--no-edit"]);

            assert_eq!(
                get_operation(&repo).await.unwrap(),
                Some(Operation::CherryPick {
                    commit: None,
                    remaining: 1
                })
            );
            continue_operation(&repo, OperationKind::CherryPick)
                .await
                .unwrap();
            assert_eq!(get_operation(&repo).await.unwrap(), None);
            assert_eq!(git(&path, ["log", "-2", "--format=%s"]), "another\ntheirs");
        });
    }

    #[test]
    fn a_cherry_pick_counts_the_commits_left_in_a_series() {
        let fx = Fixture::new();
        let path = fx.create_diverged_repo("op-cherry-picks", &text_conflict(), None);
        git(&path, ["checkout", "-q", "side"]);
        commit_files(&path, &files([("g.txt", "g\n")]), "another");
        commit_files(&path, &files([("h.txt", "h\n")]), "and another");
        git(&path, ["checkout", "-q", "main"]);
        try_git(&path, ["cherry-pick", "main..side"]);
        smol::block_on(async {
            let repo = fx.repos.open("op-cherry-picks").await.unwrap();
            let operation = get_operation(&repo).await.unwrap();
            assert!(
                matches!(
                    &operation,
                    Some(Operation::CherryPick {
                        commit: Some(CommitName { subject, .. }),
                        remaining: 2,
                    }) if subject == "theirs"
                ),
                "{operation:?}"
            );
        });
    }

    // a revert

    #[test]
    fn a_revert_names_the_commit_reverted() {
        let fx = Fixture::new();
        let path = fx.create_repo("op-revert");
        commit_files(&path, &files([("f.txt", "one\n")]), "first");
        commit_files(&path, &files([("f.txt", "two\n")]), "second");
        commit_files(&path, &files([("f.txt", "three\n")]), "third");
        try_git(&path, ["revert", "--no-edit", "HEAD~"]);
        smol::block_on(async {
            let repo = fx.repos.open("op-revert").await.unwrap();
            assert_eq!(
                get_operation(&repo).await.unwrap(),
                Some(Operation::Revert {
                    commit: Some(CommitName {
                        sha: git(&path, ["rev-parse", "--short", "HEAD~"]),
                        subject: "second".into(),
                    }),
                    remaining: 0,
                })
            );
        });
    }

    // a stash that conflicts as it's popped

    #[test]
    fn a_stash_that_conflicts_as_its_popped_leaves_conflicts_to_resolve_without_an_operation() {
        let fx = Fixture::new();
        let path = fx.create_repo("op-stash");
        commit_files(&path, &files([("f.txt", "one\n")]), "first");
        fs::write(path.join("f.txt"), "stashed\n").unwrap();
        git(&path, ["stash", "push", "-q"]);
        commit_files(&path, &files([("f.txt", "committed\n")]), "second");
        // TS popped it with `popStash`, of a feature not ported yet, which pops it with git.
        try_git(&path, ["stash", "pop", "-q"]);
        smol::block_on(async {
            let repo = fx.repos.open("op-stash").await.unwrap();

            assert_eq!(get_operation(&repo).await.unwrap(), None);
            assert_eq!(
                conflict_text(&path),
                "<<<<<<< Updated upstream\ncommitted\n=======\nstashed\n>>>>>>> Stashed changes\n"
            );
        });
    }

    // Not in TS: the parsing the tests above go through only in part.

    #[test]
    fn steps_are_read_from_gits_counters() {
        assert_eq!(
            steps(Some("1"), Some("2")),
            Some(Steps { step: 1, total: 2 })
        );
        assert_eq!(
            steps(Some("2"), Some("2")),
            Some(Steps { step: 2, total: 2 })
        );
        assert_eq!(steps(Some("0"), Some("2")), None);
        assert_eq!(steps(Some("3"), Some("2")), None);
        assert_eq!(steps(Some("1.5"), Some("2")), None);
        assert_eq!(steps(None, Some("2")), None);
        assert_eq!(steps(Some(""), Some("2")), None);
        assert_eq!(steps(Some("1"), None), None);
    }

    #[test]
    fn rebase_state_tells_a_rebase_from_git_am() {
        let fx = Fixture::new();
        let path = fx.create_repo("rebase-state");
        let git_dir = path.join(".git");
        smol::block_on(async {
            assert_eq!(rebase_state(&git_dir).await, RebaseState::default());
            fs::create_dir(git_dir.join("rebase-apply")).unwrap();
            assert_eq!(
                rebase_state(&git_dir).await,
                RebaseState {
                    rebasing: true,
                    applying: false
                }
            );
            fs::write(git_dir.join("rebase-apply").join("applying"), "").unwrap();
            assert_eq!(
                rebase_state(&git_dir).await,
                RebaseState {
                    rebasing: false,
                    applying: true
                }
            );
        });
    }

    #[test]
    fn read_state_trims_a_file_and_has_none_with_nothing_in_it() {
        let fx = Fixture::new();
        let file = |name: &str, contents: &str| {
            let file = fx.root().join(name);
            fs::write(&file, contents).unwrap();
            file
        };
        smol::block_on(async {
            assert_eq!(
                read_state(file("head", " abc\n")).await.as_deref(),
                Some("abc")
            );
            assert_eq!(read_state(file("empty", "")).await, None);
            assert_eq!(read_state(file("blank", " \n\t")).await, None);
            assert_eq!(read_state(fx.root().join("missing")).await, None);
        });
    }

    #[test]
    fn todo_lines_leave_out_comments_and_blanks() {
        assert_eq!(
            todo_lines(Some("pick a one\n# comment\n\n  indented\nrevert b two")),
            ["pick a one", "revert b two"]
        );
        assert!(todo_lines(None).is_empty());
    }

    #[test]
    fn asks_for_message_knows_which_lines_need_an_editor() {
        for line in [
            "r abc subject",
            "reword abc subject",
            "squash abc subject",
            "fixup -c abc subject",
            "merge -c abc side",
            "m abc side",
        ] {
            assert!(asks_for_message(line), "{line}");
        }
        for line in [
            "pick abc subject",
            "fixup abc subject",
            "fixup -C abc subject",
            "merge -C abc side",
            "exec make",
            "squashed abc subject",
            "reword",
        ] {
            assert!(!asks_for_message(line), "{line}");
        }
    }

    #[test]
    fn squashing_is_a_squash_or_a_fixup() {
        for line in [
            "s abc subject",
            "squash abc subject",
            "f abc subject",
            "fixup -C abc",
        ] {
            assert!(squashing(line), "{line}");
        }
        for line in [
            "pick abc subject",
            "reword abc subject",
            "fixups abc",
            "squash",
        ] {
            assert!(!squashing(line), "{line}");
        }
    }

    #[test]
    fn its_futures_are_send() {
        fn send<T: Send>(_: T) {}
        #[allow(dead_code)]
        fn check(repo: &Repo, git_dir: &Path) {
            send(get_operation(repo));
            send(continue_operation(repo, OperationKind::Merge));
            send(abort_operation(repo, OperationKind::Merge));
            send(operation_under_way(git_dir));
        }
    }
}
