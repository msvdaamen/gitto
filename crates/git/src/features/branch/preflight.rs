//! What merging and rebasing a branch check before they start, which is the same for both: the
//! input, nothing else under way, HEAD on the branch the user saw, and the other branch there.

use std::path::Path;

use super::commands::{branch_target, ref_sha};
use crate::core::errors::{Error, Result};
use crate::core::repo::{GitCommand, current_branch, has_conflicts, resolve_ref};
use crate::features::operation::commands::operation_under_way;
use crate::features::refs::parse::RefName;

/// Which of the two `preflight` checks for, which words what stops it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Verb {
    Merge,
    Rebase,
}

impl Verb {
    /// "merge", as in "then merge."
    fn word(self) -> &'static str {
        match self {
            Verb::Merge => "merge",
            Verb::Rebase => "rebase",
        }
    }

    /// The error for what stops it: `Error::MergeBlocked` or `Error::RebaseBlocked`.
    fn blocked(self, message: String) -> Error {
        match self {
            Verb::Merge => Error::MergeBlocked(message),
            Verb::Rebase => Error::RebaseBlocked(message),
        }
    }
}

/// The branch `ref` (a full ref name, local or remote) names, by its kind and short name, to merge
/// into or rebase `branch`, the one checked out as the user saw it. Fails with
/// `Error::InvalidInput` if `branch` is empty or `ref` isn't a branch, as the UI's input is
/// checked.
pub(super) fn target(reference: &str, branch: &str) -> Result<RefName> {
    if branch.is_empty() {
        return Err(Error::InvalidInput("The branch name is empty.".to_owned()));
    }
    branch_target(reference)
}

/// What `preflight` found.
#[derive(Debug)]
pub(super) struct Checked {
    /// The commit the branch to merge, or rebase onto, is at.
    pub sha: String,
    /// The commit HEAD is at; `None` on a branch without commits yet.
    pub head: Option<String>,
}

/// Checks, through `run` (a command of `repo.exclusive`, the worktree's git directory `git_dir`),
/// that the branch `target`, named `ref` in full, can be merged into, or `branch` rebased onto it.
/// Fails with `Error::MergeBlocked` or `Error::RebaseBlocked`, as `verb` says, while an operation
/// (a merge, a rebase…) or conflicts are under way, HEAD is detached, `ref` is gone, or it's
/// `branch` itself; with `Error::RepositoryChanged` if another branch than `branch` is checked out
/// by now.
pub(super) async fn preflight(
    run: &GitCommand,
    git_dir: &Path,
    reference: &str,
    target: &RefName,
    branch: &str,
    verb: Verb,
) -> Result<Checked> {
    let (operation, conflicted, current, sha, head) = futures::try_join!(
        async { Ok::<_, Error>(operation_under_way(git_dir).await) },
        has_conflicts(run),
        current_branch(run),
        // That ref itself, rather than git reading `ref` as a revision, like `refs/heads/main~3`.
        ref_sha(run, reference),
        resolve_ref(run, "HEAD"),
    )?;
    let word = verb.word();
    if let Some(operation) = operation {
        return Err(verb.blocked(format!(
            "A {} is under way. Finish or abort it, then {word}.",
            operation.name()
        )));
    }
    if conflicted {
        return Err(verb.blocked(format!(
            "Some files have conflicts. Resolve them, then {word}."
        )));
    }
    let Some(current) = current else {
        return Err(verb.blocked(format!(
            "HEAD is detached: check out a branch to {}.",
            match verb {
                Verb::Merge => "merge into it",
                Verb::Rebase => "rebase it",
            }
        )));
    };
    if current != branch {
        return Err(Error::RepositoryChanged(format!(
            "Switched from {branch} to {current} meanwhile, so nothing was {word}d."
        )));
    }
    let Some(sha) = sha else {
        return Err(verb.blocked(format!("{} no longer exists.", target.name)));
    };
    if reference == format!("refs/heads/{branch}") {
        return Err(verb.blocked(match verb {
            Verb::Merge => format!("Can't merge {branch} into itself."),
            Verb::Rebase => format!("Can't rebase {branch} onto itself."),
        }));
    }
    Ok(Checked { sha, head })
}
