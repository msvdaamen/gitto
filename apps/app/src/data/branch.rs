//! Changing the branches: creating, switching, deleting, merging and rebasing. Ported from
//! `git/queries/branch.ts`.

use std::future::Future;

use gitto_git::ApiError;
use gitto_git::features::branch::commands::{create_branch, switch_branch};
use gitto_git::features::branch::delete::delete_branch;
use gitto_git::features::branch::merge::merge_branch;
use gitto_git::features::branch::rebase::rebase_branch;
use gitto_git::features::branch::schema::{MergeOutcome, RebaseOutcome};
use gitto_git::features::status::schema::Uncommitted;
use gitto_git::input::{BranchName, BranchRef, NonEmpty, RepositoryId};
use gpui_kit::{App, AppContext as _, AsyncApp, Task};

use super::call::{call, parse};
use super::client::{QueryClient, QueryError};
use super::keys;
use super::opaque::Opaque;
use super::operation::{OneAtATime, OperationState, Operations, RepositoryOperation, reloading};
use gitto_ui::git::status::{branch_name, is_checked_out};

const CREATE: &str = "branch";
const SWITCH: &str = "switch";
const MERGE: &str = "merge";
const REBASE: &str = "rebase";
const DELETE: &str = "delete-branch";
/// Adding a worktree (see `worktree::worktree_actions`), which checks out a branch there, or
/// creates one.
pub const ADD_WORKTREE: &str = "add-worktree";

/// Whether one of the operations that change the branches is running on the repository `id`:
/// creating one at HEAD, a switch of branches ([`switch_branch_operation`],
/// [`create_branch_from_operation`]), a merge, a rebase, deleting one, or adding a worktree, which
/// checks one out (`useBranchesChanging`).
pub fn branches_changing(id: &str, cx: &App) -> bool {
    [CREATE, SWITCH, MERGE, REBASE, DELETE, ADD_WORKTREE]
        .iter()
        .any(|name| Operations::state(name, id, cx).running)
}

/// One of the operations that change the branches, which doesn't run while any of them does: a
/// switch would switch away from the branch merged into before the user saw how the merge went,
/// say, or check out a branch being deleted, which git then wouldn't delete. Whether it can't run
/// now, as one of them is running, is [`is_blocked`](Self::is_blocked) (`oneBranchChangeAtATime`).
pub struct BranchChange<T, R = ()> {
    operation: OneAtATime<T, R>,
}

impl<T, R> Clone for BranchChange<T, R> {
    fn clone(&self) -> Self {
        BranchChange {
            operation: self.operation.clone(),
        }
    }
}

impl<T: 'static, R: 'static> BranchChange<T, R> {
    /// `operation`, as one of the changes of the branches: the data layer's own, or a test's.
    pub(crate) fn new(operation: RepositoryOperation<T, R>) -> Self {
        BranchChange {
            operation: operation.one_at_a_time(branches_changing),
        }
    }

    /// Runs it on the repository `id`, unless a change of the branches is running there (see
    /// `RepositoryOperation::run`).
    pub fn run(
        &self,
        id: &RepositoryId,
        input: T,
        cx: &mut App,
    ) -> Option<futures::channel::oneshot::Receiver<Result<R, QueryError>>> {
        self.operation.run(id, input, cx)
    }

    /// Whether it's running on the repository `id`, and why it last failed.
    pub fn state(&self, id: &str, cx: &App) -> OperationState {
        self.operation.state(id, cx)
    }

    /// Forgets why it last failed on the repository `id`.
    pub fn dismiss(&self, id: &str, cx: &mut App) {
        self.operation.dismiss(id, cx)
    }

    /// Whether it can't run on the repository `id` now, as a change of the branches is running.
    pub fn is_blocked(&self, id: &str, cx: &App) -> bool {
        branches_changing(id, cx)
    }

    pub fn operation(&self) -> &OneAtATime<T, R> {
        &self.operation
    }
}

/// Creates a branch, by its name, at HEAD and switches to it, taking the uncommitted changes
/// along; whether it's running, and why it last failed (`useCreateBranch`). It counts as running
/// until the repository has reloaded, so the toolbar names the new branch once it's done. Not
/// while another change of the branches runs.
pub fn create_branch_operation() -> BranchChange<String> {
    BranchChange::new(RepositoryOperation::new(
        CREATE,
        |id, name: String, cx: &mut App| {
            let action = call(cx, "branch.create", &id, move |repo| async move {
                create_branch(&repo, &parse::<BranchName>(name)?, None).await
            });
            cx.spawn(async move |cx| {
                cx.background_spawn(action).await?;
                cx.update(|cx| QueryClient::invalidate(&keys::repository(&id), cx))
                    .await;
                Ok(())
            })
        },
    ))
}

/// Switches to a branch, by its full ref name: for a remote one, the local branch tracking it, or
/// a new one that does (`useSwitchBranch`). The uncommitted changes come along, or are kept in the
/// stash when they conflict with it. Whether it's running, and why it last failed. It counts as
/// running until the repository has reloaded, also after a failure: it may have switched and
/// stashed. Switching to the checked-out branch does nothing, rather than reloading the whole
/// repository. Not while another change of the branches runs.
pub fn switch_branch_operation() -> BranchChange<String> {
    BranchChange::new(RepositoryOperation::new(
        SWITCH,
        |id, reference: String, cx: &mut App| -> Task<Result<(), QueryError>> {
            let status = QueryClient::get_data::<Opaque<Uncommitted>>(&keys::status(&id), cx);
            if status.is_some_and(|status| is_checked_out(&status.head, &reference)) {
                return Task::ready(Ok(()));
            }
            let action = call(cx, "branch.switch", &id, move |repo| async move {
                switch_branch(&repo, &parse::<BranchRef>(reference)?).await
            });
            cx.spawn(async move |cx| reloading(&id, action, cx).await)
        },
    ))
}

/// A branch to make, `name`, at the branch `from`, by its full ref name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewBranchFrom {
    pub name: String,
    pub from: String,
}

/// Creates a branch at another (see [`NewBranchFrom`]) and switches to it, taking the
/// uncommitted changes along, or keeping them in the stash when they conflict with it
/// (`useCreateBranchFrom`). That's a switch of branches, like [`switch_branch_operation`]'s: it's
/// shown as one. It counts as running until the repository has reloaded, also after a failure: it
/// may have switched and stashed. Not while another change of the branches runs.
pub fn create_branch_from_operation() -> BranchChange<NewBranchFrom> {
    BranchChange::new(RepositoryOperation::new(
        SWITCH,
        |id, input: NewBranchFrom, cx: &mut App| {
            let action = call(cx, "branch.create", &id, move |repo| async move {
                let name = parse::<BranchName>(input.name)?;
                let from = parse::<BranchRef>(input.from)?;
                create_branch(&repo, &name, Some(&from)).await
            });
            cx.spawn(async move |cx| reloading(&id, action, cx).await)
        },
    ))
}

/// Whether a switch of branches (see [`switch_branch_operation`] and
/// [`create_branch_from_operation`]) is running, and why it last failed (`useSwitchBranchState`).
pub fn switch_branch_state(id: &str, cx: &App) -> OperationState {
    Operations::state(SWITCH, id, cx)
}

/// Deletes a branch, by its full ref name, with the commits only it has: a local one here only; a
/// remote one on its remote, and the local branches tracking it with it (`useDeleteBranch`).
/// Whether it's running, and why it last failed. It counts as running until the repository has
/// reloaded, also after a failure: the remote one may be gone though the local ones aren't. Not
/// while another change of the branches runs.
pub fn delete_branch_operation() -> BranchChange<String> {
    BranchChange::new(RepositoryOperation::new(
        DELETE,
        |id, reference: String, cx: &mut App| {
            let action = call(cx, "branch.delete", &id, move |repo| async move {
                delete_branch(&repo, &parse::<BranchRef>(reference)?).await
            });
            cx.spawn(async move |cx| reloading(&id, action, cx).await)
        },
    ))
}

/// Whether a branch is being deleted (see [`delete_branch_operation`]), and why that last failed
/// (`useDeleteBranchState`).
pub fn delete_branch_state(id: &str, cx: &App) -> OperationState {
    Operations::state(DELETE, id, cx)
}

/// A merge of the branch `reference`, by its full ref name, into the checked-out one, `into`, as
/// the user saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Merge {
    pub reference: String,
    pub into: String,
}

/// Merges a branch into the checked-out one (see [`Merge`]); the merge's outcome, or why it failed
/// (`useMergeBranch`). One that changed nothing fails, saying so: the branch had every commit of
/// it already. One that stopped at conflicts goes through, as `Conflicts`, left for the user to
/// resolve. It counts as running until the repository has reloaded, also after a failure: it may
/// have stopped partway; it isn't reloaded after one that changed nothing. Not while another
/// change of the branches runs.
pub fn merge_branch_operation() -> BranchChange<Merge, MergeOutcome> {
    BranchChange::new(RepositoryOperation::new(
        MERGE,
        |id, input: Merge, cx: &mut App| {
            let up_to_date = format!(
                "Already up to date: {} has every commit of {}.",
                input.into,
                branch_name(&input.reference)
            );
            let action = call(cx, "branch.merge", &id, move |repo| async move {
                let reference = parse::<BranchRef>(input.reference)?;
                let into = parse::<NonEmpty>(input.into)?;
                merge_branch(&repo, &reference, &into).await
            });
            cx.spawn(async move |cx| {
                integrating(
                    &id,
                    action,
                    |outcome| *outcome == MergeOutcome::UpToDate,
                    up_to_date,
                    cx,
                )
                .await
            })
        },
    ))
}

/// Runs `action`, a merge or a rebase on the repository `id`, then reloads the repository, also
/// after a failure: it may have stopped partway. Not after one that changed nothing
/// (`is_up_to_date`), which fails instead, saying `up_to_date`: the branch had every commit
/// already.
pub(crate) async fn integrating<T: Send + 'static>(
    id: &str,
    action: impl Future<Output = Result<T, ApiError>> + Send + 'static,
    is_up_to_date: impl Fn(&T) -> bool,
    up_to_date: String,
    cx: &mut AsyncApp,
) -> Result<T, QueryError> {
    let reload =
        |cx: &mut AsyncApp| cx.update(|cx| QueryClient::invalidate(&keys::repository(id), cx));
    let outcome = match cx.background_spawn(action).await {
        Ok(outcome) => outcome,
        Err(error) => {
            reload(cx).await;
            return Err(error.into());
        }
    };
    if is_up_to_date(&outcome) {
        return Err(QueryError::msg(up_to_date));
    }
    reload(cx).await;
    Ok(outcome)
}

/// Whether a merge (see [`merge_branch_operation`]) is running, and why it last failed
/// (`useMergeBranchState`).
pub fn merge_branch_state(id: &str, cx: &App) -> OperationState {
    Operations::state(MERGE, id, cx)
}

/// A rebase of the checked-out branch, `branch`, as the user saw it, onto the branch `reference`,
/// by its full ref name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rebase {
    pub reference: String,
    pub branch: String,
}

/// Rebases the checked-out branch onto another (see [`Rebase`]); the rebase's outcome, or why it
/// failed (`useRebaseBranch`). One that changed nothing fails, saying so: the branch was on top of
/// the other already. One that stopped at conflicts goes through, as `Conflicts`, left for the
/// user to resolve and continue. It counts as running until the repository has reloaded, also
/// after a failure: it may have stopped partway; it isn't reloaded after one that changed nothing.
/// Not while another change of the branches runs.
pub fn rebase_branch_operation() -> BranchChange<Rebase, RebaseOutcome> {
    BranchChange::new(RepositoryOperation::new(
        REBASE,
        |id, input: Rebase, cx: &mut App| {
            let up_to_date = format!(
                "Already up to date: {} has every commit of {}.",
                input.branch,
                branch_name(&input.reference)
            );
            let action = call(cx, "branch.rebase", &id, move |repo| async move {
                let reference = parse::<BranchRef>(input.reference)?;
                let branch = parse::<NonEmpty>(input.branch)?;
                rebase_branch(&repo, &reference, &branch).await
            });
            cx.spawn(async move |cx| {
                integrating(
                    &id,
                    action,
                    |outcome| *outcome == RebaseOutcome::UpToDate,
                    up_to_date,
                    cx,
                )
                .await
            })
        },
    ))
}

/// Whether a rebase (see [`rebase_branch_operation`]) is running, and why it last failed
/// (`useRebaseBranchState`).
pub fn rebase_branch_state(id: &str, cx: &App) -> OperationState {
    Operations::state(REBASE, id, cx)
}

#[cfg(test)]
mod tests {
    use futures::FutureExt as _;
    use gitto_git::features::status::schema::{Head, StatusCounts, WorkingTreeFiles};
    use gpui_kit::TestAppContext;

    use super::*;

    fn repository_id() -> RepositoryId {
        RepositoryId::parse("01890a5d-ac96-774b-bcce-b302099a8057").unwrap()
    }

    fn init(cx: &mut TestAppContext) {
        cx.update(crate::data::init);
    }

    fn invalidated(cx: &mut TestAppContext) -> Vec<crate::data::QueryKey> {
        cx.update(|cx| cx.global::<QueryClient>().invalidated.clone())
    }

    #[gpui_kit::test]
    fn fails_a_merge_that_changed_nothing_without_reloading(cx: &mut TestAppContext) {
        init(cx);
        let result = cx.update(|cx| {
            cx.spawn(async move |cx| {
                integrating(
                    "repo",
                    async { Ok::<_, ApiError>(MergeOutcome::UpToDate) },
                    |outcome| *outcome == MergeOutcome::UpToDate,
                    "Already up to date: main has every commit of feature.".to_owned(),
                    cx,
                )
                .await
            })
        });
        cx.run_until_parked();
        let error = result.now_or_never().unwrap().unwrap_err();
        assert_eq!(
            error.message(),
            "Already up to date: main has every commit of feature."
        );
        assert!(invalidated(cx).is_empty());
    }

    #[gpui_kit::test]
    fn reloads_after_a_merge_that_stopped_at_conflicts(cx: &mut TestAppContext) {
        init(cx);
        let result = cx.update(|cx| {
            cx.spawn(async move |cx| {
                integrating(
                    "repo",
                    async { Ok::<_, ApiError>(MergeOutcome::Conflicts) },
                    |outcome| *outcome == MergeOutcome::UpToDate,
                    String::new(),
                    cx,
                )
                .await
            })
        });
        cx.run_until_parked();
        assert_eq!(
            result.now_or_never().unwrap().unwrap(),
            MergeOutcome::Conflicts
        );
        assert_eq!(invalidated(cx), [keys::repository("repo")]);
    }

    #[gpui_kit::test]
    fn switching_to_the_checked_out_branch_does_nothing(cx: &mut TestAppContext) {
        init(cx);
        let id = repository_id();
        let status = Uncommitted {
            head: Head::Branch {
                name: "main".into(),
                sha: "abc".into(),
            },
            upstream: None,
            ahead: 0,
            behind: 0,
            counts: StatusCounts {
                files: 0,
                staged: 0,
                unstaged: 0,
                conflicted: 0,
            },
            changes: WorkingTreeFiles {
                staged: vec![],
                unstaged: vec![],
                uncounted: false,
                marker_free: vec![],
            },
            version: "v1".into(),
        };
        cx.update(|cx| QueryClient::set_data(&keys::status(&id), Opaque::new(status), cx));
        let switch = switch_branch_operation();
        let done = cx
            .update(|cx| switch.run(&id, "refs/heads/main".into(), cx))
            .unwrap();
        cx.run_until_parked();
        assert!(done.now_or_never().unwrap().unwrap().is_ok());
        assert!(invalidated(cx).is_empty());
    }

    #[gpui_kit::test]
    fn changes_one_branch_at_a_time(cx: &mut TestAppContext) {
        init(cx);
        let id = repository_id();
        let (finish, finished) = futures::channel::oneshot::channel::<()>();
        let finished = std::cell::Cell::new(Some(finished));
        // A deletion that runs until the test says.
        let delete = BranchChange::new(RepositoryOperation::new(
            DELETE,
            move |_, (), cx: &mut App| {
                let finished = finished.take().unwrap();
                cx.background_spawn(async move {
                    let _ = finished.await;
                    Ok(())
                })
            },
        ));
        let merge = merge_branch_operation();
        let _running = cx.update(|cx| delete.run(&id, (), cx)).unwrap();
        assert!(cx.update(|cx| merge.is_blocked(&id, cx)));
        let input = Merge {
            reference: "refs/heads/feature".into(),
            into: "main".into(),
        };
        assert!(cx.update(|cx| merge.run(&id, input, cx)).is_none());
        finish.send(()).unwrap();
        cx.run_until_parked();
        assert!(!cx.update(|cx| merge.is_blocked(&id, cx)));
    }
}
