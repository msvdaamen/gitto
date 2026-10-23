//! The stashes: listing them, what each changed, and stashing, popping and dropping. Ported from
//! `git/queries/stash.ts`.

use std::sync::Arc;

use gitto_git::features::stash::commands::{drop_stash, list_stashes, pop_stash, push_stash};
use gitto_git::features::stash::schema::Stash;
use gitto_git::input::{FullSha, RepositoryId};
use gpui_kit::{App, Context};

use super::call::{call, loads, parse};
use super::client::QueryOptions;
use super::diff::{ChangedFiles, FilesOf, ShownFiles, changed_files_query};
use super::keys;
use super::operation::{OneAtATime, Operations, RepositoryOperation, reloading};
use super::query::Query;

/// The stashes, newest first. A reload equal to what was there keeps it, so one that brings
/// nothing new (the refs changed elsewhere, say) doesn't lay out and render the history again
/// (TS's `reconcile: "sha"`).
pub fn stashes_options(id: &RepositoryId, cx: &App) -> QueryOptions<Arc<[Stash]>> {
    QueryOptions::new(
        keys::stashes(id),
        loads(cx, "stash.list", id, |repo| async move {
            Ok(list_stashes(&repo).await?.into())
        }),
    )
}

/// The stashes' query (`useStashes`).
pub fn stashes_query<V: 'static>(id: &RepositoryId, cx: &mut Context<V>) -> Query<Arc<[Stash]>> {
    Query::new(stashes_options(id, cx), cx)
}

/// The files a stash changed compared to the commit it was made on, untracked ones included
/// (`useStashFiles`).
pub fn stash_files_query<V: 'static>(
    id: &RepositoryId,
    sha: &str,
    cx: &mut Context<V>,
) -> Query<ChangedFiles, ShownFiles> {
    changed_files_query(id, sha, FilesOf::Stash, cx)
}

const STASH: &str = "stash";
const POP: &str = "pop";
const POP_PICKED: &str = "pop-picked";
const DROP: &str = "drop-stash";

/// Stashing every change, popping a stash, and dropping one (`useStashActions`); whether each is
/// running, and why it last failed. Each counts as running until the repository has reloaded, so
/// the stashes and changes on show are never ones from before it. One runs at a time
/// ([`is_pending`](Self::is_pending)): the toolbar's pop names the stash it pops, which has to be
/// the newest. A pop of the newest, from the toolbar, and one of the stash picked in a list are
/// separate, to show each where it was started.
#[derive(Clone)]
pub struct StashActions {
    pub stash: OneAtATime<()>,
    /// Pops the newest stash, by its SHA.
    pub pop: OneAtATime<String>,
    /// Pops the stash picked in a list, the newest or an older one.
    pub pop_picked: OneAtATime<String>,
    /// Drops a stash, by its SHA.
    pub drop: OneAtATime<String>,
}

impl StashActions {
    /// Whether any of them is running on the repository `id`.
    pub fn is_pending(id: &str, cx: &App) -> bool {
        [STASH, POP, POP_PICKED, DROP]
            .iter()
            .any(|name| Operations::state(name, id, cx).running)
    }
}

/// The operations of [`StashActions`]. Each reloads the repository after, also after a failure:
/// a pop can conflict.
pub fn stash_actions() -> StashActions {
    let popping = |id: RepositoryId, sha: String, cx: &mut App| {
        let action = call(cx, "stash.pop", &id, move |repo| async move {
            pop_stash(&repo, &parse::<FullSha>(sha)?).await
        });
        cx.spawn(async move |cx| reloading(&id, action, cx).await)
    };
    let busy = |id: &str, cx: &App| StashActions::is_pending(id, cx);
    StashActions {
        stash: RepositoryOperation::new(STASH, |id, (), cx: &mut App| {
            let action = call(cx, "stash.push", &id, |repo| async move {
                push_stash(&repo).await
            });
            cx.spawn(async move |cx| reloading(&id, action, cx).await)
        })
        .one_at_a_time(busy),
        pop: RepositoryOperation::new(POP, popping).one_at_a_time(busy),
        pop_picked: RepositoryOperation::new(POP_PICKED, popping).one_at_a_time(busy),
        drop: RepositoryOperation::new(DROP, |id, sha: String, cx: &mut App| {
            let action = call(cx, "stash.drop", &id, move |repo| async move {
                drop_stash(&repo, &parse::<FullSha>(sha)?).await
            });
            cx.spawn(async move |cx| reloading(&id, action, cx).await)
        })
        .one_at_a_time(busy),
    }
}
