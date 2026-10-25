//! The merge, rebase, cherry-pick, revert or `git am` under way, and continuing or aborting it.
//! Ported from `git/queries/progress.ts`.

use gitto_git::features::operation::schema::{Operation, OperationKind};
use gitto_git::input::RepositoryId;
use gpui_kit::{App, Context};

use super::call::{call, loads};
use super::client::QueryOptions;
use super::keys;
use super::operation::{RepositoryOperation, reloading};
use super::query::Query;

/// The merge, rebase, cherry-pick, revert or `git am` under way in the repository; `None` if
/// none.
pub fn operation_in_progress_options(
    id: &RepositoryId,
    cx: &App,
) -> QueryOptions<Option<Operation>> {
    QueryOptions::new(keys::operation(id), loads(cx, id, |repo| repo.operation()))
}

/// The operation under way's query (`useOperationInProgress`).
pub fn operation_in_progress_query<V: 'static>(
    id: &RepositoryId,
    cx: &mut Context<V>,
) -> Query<Option<Operation>> {
    Query::new(operation_in_progress_options(id, cx), cx)
}

/// Continuing and aborting the operation under way (`useOperationActions`); each says whether
/// it's running, and why it last failed. Each counts as running until the repository has
/// reloaded, so the operation on show is never the one from before: a rebase that goes on stops
/// at the next commit's conflicts, say.
#[derive(Clone)]
pub struct OperationActions {
    /// `continue`, which Rust keeps for itself.
    pub proceed: RepositoryOperation<OperationKind>,
    pub abort: RepositoryOperation<OperationKind>,
}

/// The operations of [`OperationActions`].
pub fn operation_actions() -> OperationActions {
    OperationActions {
        proceed: RepositoryOperation::new("continue", |id, kind, cx: &mut App| {
            let action = call(cx, &id, move |repo| repo.continue_operation(kind));
            cx.spawn(async move |cx| reloading(&id, action, cx).await)
        }),
        abort: RepositoryOperation::new("abort", |id, kind, cx: &mut App| {
            let action = call(cx, &id, move |repo| repo.abort_operation(kind));
            cx.spawn(async move |cx| reloading(&id, action, cx).await)
        }),
    }
}
