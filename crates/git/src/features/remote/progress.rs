//! What a merge, rebase or `git am` left unfinished in a repository, and what to tell the user
//! about a pull that stopped at it.

use std::path::Path;

use super::blockers::{Action, detached_head};
use crate::core::errors::{Error, GitError, Result};
use crate::core::repo::{GitCommand, has_conflicts, ref_exists, succeeds};
use crate::features::operation::commands::rebase_state;

/// What a merge, rebase or `git am` left unfinished in a repository.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Progress {
    pub conflicts: bool,
    pub merging: bool,
    pub rebasing: bool,
    /// Applying patches with `git am`, which keeps its state where some rebases do.
    pub applying: bool,
}

/// Whether `state` has anything unfinished, which git won't merge or rebase over.
pub fn in_progress(state: &Progress) -> bool {
    state.conflicts || state.merging || state.rebasing || state.applying
}

/// Why HEAD, detached in the worktree whose git directory is `git_dir`, can't be pulled into or
/// pushed: a rebase may have stopped there.
pub async fn why_detached(git_dir: &Path, action: Action) -> String {
    detached_head(action, rebase_state(git_dir).await.rebasing)
}

/// What's unfinished in the repository, read at once.
pub async fn progress(run: &GitCommand, git_dir: &Path) -> Result<Progress> {
    let (conflicts, merging, state) = futures::join!(
        has_conflicts(run),
        ref_exists(run, "MERGE_HEAD"),
        rebase_state(git_dir),
    );
    Ok(Progress {
        conflicts: conflicts?,
        merging: merging?,
        rebasing: state.rebasing,
        applying: state.applying,
    })
}

/// Whether `ancestor` is in `rev`'s history; fails if git couldn't tell.
async fn is_ancestor(run: &GitCommand, ancestor: &str, rev: &str) -> Result<bool> {
    succeeds(run, ["merge-base", "--is-ancestor", ancestor, rev]).await
}

/// Why the merge or rebase `args`, which `failure` is of if it failed, left the pull for the user
/// to finish, now that it's done, as an `Error::PullInterrupted`; `None` if it didn't. Even one
/// that went through can have: putting back local changes it stashed can conflict, and
/// `branch.<name>.mergeOptions` can stop a merge before committing, or squash it, so what was
/// merged isn't in HEAD.
pub async fn interruption<S: AsRef<str>>(
    run: &GitCommand,
    git_dir: &Path,
    args: &[S],
    upstream: &str,
    failure: Option<&GitError>,
) -> Result<Option<Error>> {
    let after = progress(run, git_dir).await?;
    // A merge that went through, or stopped at conflicts, without a merge to commit: squashed, if
    // what it merged isn't in HEAD.
    let merged = args.last().map_or("", AsRef::as_ref);
    let squashing = args.first().is_some_and(|arg| arg.as_ref() == "merge")
        && !after.merging
        && (failure.is_none() || after.conflicts)
        && !is_ancestor(run, merged, "HEAD").await?;
    if !after.conflicts && !after.merging && !after.rebasing && !squashing {
        return Ok(None);
    }
    let name = upstream
        .strip_prefix("refs/remotes/")
        .or_else(|| upstream.strip_prefix("refs/heads/"))
        .unwrap_or(upstream);
    Ok(Some(interrupted_error(name, &after, squashing, failure)))
}

/// Says where the pull stopped, and what to do to finish it.
fn interrupted_error(
    upstream: &str,
    progress: &Progress,
    squashing: bool,
    failure: Option<&GitError>,
) -> Error {
    let Progress {
        conflicts,
        merging,
        rebasing,
        ..
    } = *progress;
    // Why git stopped, when it wasn't at conflicts: e.g. an untracked file in the way of a commit
    // a rebase replays, or a hook that turned a merge commit down.
    let reason = failure
        .map(|failure| failure.without_hints().message)
        .filter(|reason| !reason.is_empty());
    let stopped = |what: &str, next: &str| match &reason {
        Some(reason) => format!("{what}:\n{reason}\nFix that, then {next}."),
        None => format!("{what}. {}.", capitalize(next)),
    };
    let message = if rebasing && !conflicts {
        stopped(
            &format!("Rebasing onto {upstream} stopped partway"),
            "continue the rebase, or abort it",
        )
    } else if rebasing {
        format!("Pulling {upstream} caused conflicts. Resolve them, then continue the rebase.")
    } else if merging && !conflicts {
        stopped(
            &format!("Merging {upstream} stopped before committing"),
            "commit the merge, or abort it",
        )
    } else if merging {
        format!("Pulling {upstream} caused conflicts. Resolve them, then commit the merge.")
    } else if squashing && !conflicts {
        format!(
            "Squashed {upstream} into the staged changes, without committing. Commit them to finish pulling."
        )
    } else if squashing {
        format!(
            "Pulling {upstream} caused conflicts. Resolve them, then commit the squashed changes."
        )
    } else {
        // The pull stashed local changes (rebase.autoStash, merge.autoStash) and they conflict.
        format!(
            "Pulled {upstream}, but your local changes conflict with it. Resolve the conflicts; your changes are also kept in the stash."
        )
    };
    Error::PullInterrupted(message)
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::test_support::conflicts::text_conflict;
    use crate::test_support::{Fixture, git};

    #[test]
    fn why_detached_says_a_rebase_is_under_way_but_not_for_git_am() {
        let fx = Fixture::new();
        let path = fx.create_repo("why-detached");
        let git_dir = path.join(".git");
        smol::block_on(async {
            fs::create_dir(git_dir.join("rebase-apply")).unwrap();
            assert_eq!(
                why_detached(&git_dir, Action::Push).await,
                "A rebase is under way. Continue or abort it, then push."
            );
            fs::write(git_dir.join("rebase-apply").join("applying"), "").unwrap();
            assert_eq!(
                why_detached(&git_dir, Action::Push).await,
                "HEAD is detached: check out a branch to push it."
            );
        });
    }

    #[test]
    fn a_merge_that_conflicted_interrupts_the_pull() {
        let fx = Fixture::new();
        let path = fx.create_merge_conflict("interrupted", None, &text_conflict());
        smol::block_on(async {
            let repo = fx.repos.open("interrupted").await.unwrap();
            let run = repo.git();
            let state = progress(&run, &path.join(".git")).await.unwrap();
            assert!(state.conflicts && state.merging && in_progress(&state));
            let error = interruption(
                &run,
                &path.join(".git"),
                &["merge", "side"],
                "refs/heads/side",
                None,
            )
            .await
            .unwrap()
            .unwrap();
            assert_eq!(
                error.to_string(),
                "Pulling side caused conflicts. Resolve them, then commit the merge."
            );

            git(&path, ["merge", "--abort"]);
            let state = progress(&run, &path.join(".git")).await.unwrap();
            assert!(!in_progress(&state));
        });
    }
}
