//! The remotes: listing them, fetching, pulling and pushing. Ported from
//! `git/queries/remote.ts`.

use futures::channel::oneshot;
use gitto_git::ApiCode;
use gitto_git::api::pull;
use gitto_git::features::remote::commands::{fetch_all, list_remotes};
use gitto_git::features::remote::push::{get_push_target, push};
use gitto_git::features::remote::schema::PushTarget;
use gitto_git::input::{NonEmpty, RepositoryId};
use gpui_kit::{App, AppContext as _, Context, Entity};

use super::call::{call, loads, parse};
use super::client::{QueryClient, QueryError, QueryOptions, StaleTime};
use super::keys;
use super::mutation::Mutation;
use super::operation::{OperationState, Operations, RepositoryOperation, reloading};
use super::query::Query;

/// The remotes, by name.
pub fn remotes_options(id: &RepositoryId, cx: &App) -> QueryOptions<Vec<String>> {
    QueryOptions::new(
        keys::remotes(id),
        loads(cx, "remote.list", id, |repo| async move {
            list_remotes(&repo.git()).await
        }),
    )
}

/// The remotes' query (`useRemotes`).
pub fn remotes_query<V: 'static>(id: &RepositoryId, cx: &mut Context<V>) -> Query<Vec<String>> {
    Query::new(remotes_options(id, cx), cx)
}

/// Fetches every remote of the repository whose id it's given (`useFetch`). The id is the
/// mutation's input (`Mutation::input`), so a fetch still running after switching repositories
/// reloads the one it fetched, and its state can be told apart from the current repository's.
pub fn fetch_mutation(cx: &mut App) -> Entity<Mutation<RepositoryId>> {
    Mutation::new(
        |id: RepositoryId, cx: &mut App| {
            let fetching = call(cx, "remote.fetch", &id, |repo| async move {
                fetch_all(&repo).await
            });
            cx.spawn(async move |cx| {
                cx.background_spawn(fetching).await?;
                // The watcher would catch the new remote refs too, but reloading right away feels
                // snappier.
                cx.update(|cx| QueryClient::invalidate(&keys::repository(&id), cx))
                    .await;
                Ok(())
            })
        },
        cx,
    )
}

/// Pulls the current branch's upstream; whether a pull is running, and why the last one failed
/// (`usePull`).
#[derive(Clone)]
pub struct PullOperation {
    operation: RepositoryOperation<()>,
}

impl PullOperation {
    /// Pulls into the repository `id`, unless it's pulling already.
    pub fn pull(
        &self,
        id: &RepositoryId,
        cx: &mut App,
    ) -> Option<oneshot::Receiver<Result<(), QueryError>>> {
        self.operation.run(id, (), cx)
    }

    pub fn state(&self, id: &str, cx: &App) -> OperationState {
        self.operation.state(id, cx)
    }

    pub fn dismiss(&self, id: &str, cx: &mut App) {
        self.operation.dismiss(id, cx)
    }
}

/// The pull of [`PullOperation`].
pub fn pull_operation() -> PullOperation {
    PullOperation {
        operation: RepositoryOperation::new("pull", |id, (), cx: &mut App| {
            let pulling = call(cx, "remote.pull", &id, |repo| async move {
                pull(&repo, None).await
            });
            cx.spawn(async move |cx| {
                let result = cx.background_spawn(pulling).await;
                // Also after a failure: a pull that stopped at conflicts still brought in the
                // upstream's commits. Not waited for: the pull is done (and its error shown)
                // before the history has reloaded.
                cx.update(|cx| {
                    let _ = QueryClient::invalidate(&keys::repository(&id), cx);
                });
                result.map_err(Into::into)
            })
        }),
    }
}

const PULL_BRANCH: &str = "pull-branch";

/// Pulls a local branch that isn't checked out, by its short name, fast-forwarding it to its
/// upstream without touching the working tree; whether that's running, and why it last failed
/// (`usePullBranch`). It counts as running until the repository has reloaded, also after a
/// failure: its remote was fetched all the same.
pub fn pull_branch_operation() -> RepositoryOperation<String> {
    RepositoryOperation::new(PULL_BRANCH, |id, branch: String, cx: &mut App| {
        let pulling = call(cx, "remote.pull", &id, move |repo| async move {
            pull(&repo, Some(&parse::<NonEmpty>(branch)?)).await
        });
        cx.spawn(async move |cx| reloading(&id, pulling, cx).await)
    })
}

/// Whether a branch that isn't checked out is being pulled (see [`pull_branch_operation`]), and
/// why that last failed (`usePullBranchState`).
pub fn pull_branch_state(id: &str, cx: &App) -> OperationState {
    Operations::state(PULL_BRANCH, id, cx)
}

/// Pushes the current branch, or force-pushes it; whether a push is running, and why the last one
/// failed (`usePush`). A push turned down as the remote branch has commits the branch doesn't fails
/// with an error [`push_rejection`] tells apart.
#[derive(Clone)]
pub struct PushOperation {
    /// Its input is whether to force.
    operation: RepositoryOperation<bool>,
}

impl PushOperation {
    pub fn push(
        &self,
        id: &RepositoryId,
        cx: &mut App,
    ) -> Option<oneshot::Receiver<Result<(), QueryError>>> {
        self.operation.run(id, false, cx)
    }

    pub fn force_push(
        &self,
        id: &RepositoryId,
        cx: &mut App,
    ) -> Option<oneshot::Receiver<Result<(), QueryError>>> {
        self.operation.run(id, true, cx)
    }

    pub fn state(&self, id: &str, cx: &App) -> OperationState {
        self.operation.state(id, cx)
    }

    pub fn dismiss(&self, id: &str, cx: &mut App) {
        self.operation.dismiss(id, cx)
    }
}

/// The push of [`PushOperation`].
pub fn push_operation() -> PushOperation {
    PushOperation {
        operation: RepositoryOperation::new("push", |id, force: bool, cx: &mut App| {
            let pushing = call(cx, "remote.push", &id, move |repo| async move {
                push(&repo, force).await
            });
            cx.spawn(async move |cx| {
                let result = cx.background_spawn(pushing).await;
                // The remote-tracking branch moved, and so did how far ahead the branch is. Not
                // waited for, as for a pull.
                cx.update(|cx| {
                    let _ = QueryClient::invalidate(&keys::repository(&id), cx);
                });
                result.map_err(Into::into)
            })
        }),
    }
}

/// Why the remote branch turned a push down (see [`push_rejection`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushRejection {
    /// Its commits are ones the branch had before a rebase or an amend, which force-pushing
    /// replaces.
    Rewritten,
    /// Its commits are others, to pull.
    Behind,
}

/// Why the remote branch turned a push down, if `error` says it has commits the branch doesn't
/// (`pushRejection`).
pub fn push_rejection(error: Option<&QueryError>) -> Option<PushRejection> {
    let error = error?;
    if error.has_code(ApiCode::PushRewritten) {
        Some(PushRejection::Rewritten)
    } else if error.has_code(ApiCode::PushBehind) {
        Some(PushRejection::Behind)
    } else {
        None
    }
}

/// Where the Push button pushes the current branch, and how many commits; or why it can't
/// (`pushTargetQuery`).
pub fn push_target_options(id: &RepositoryId, cx: &App) -> QueryOptions<PushTarget> {
    QueryOptions::new(
        keys::push_target(id),
        loads(cx, "remote.pushTarget", id, |repo| async move {
            get_push_target(&repo).await
        }),
    )
    // Reloaded when the refs or config change (it's under `keys::repository`), not on window
    // focus.
    .stale_time(StaleTime::Infinite)
}

/// The push target's query.
pub fn push_target_query<V: 'static>(id: &RepositoryId, cx: &mut Context<V>) -> Query<PushTarget> {
    Query::new(push_target_options(id, cx), cx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tells_why_a_push_was_turned_down() {
        let rejected = |forcible: bool| {
            QueryError::from(gitto_git::Error::PushRejected {
                message: "Rejected.".to_owned(),
                forcible,
            })
        };
        assert_eq!(
            push_rejection(Some(&rejected(true))),
            Some(PushRejection::Rewritten)
        );
        assert_eq!(
            push_rejection(Some(&rejected(false))),
            Some(PushRejection::Behind)
        );
        assert_eq!(push_rejection(Some(&QueryError::msg("Offline."))), None);
        assert_eq!(push_rejection(None), None);
    }
}
