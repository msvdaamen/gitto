//! Committing, rewording, and what's read of a commit. Ported from `git/queries/commit.ts`.

use std::future::Future;

use gitto_git::ApiError;
use gitto_git::features::commit::commands::{
    create_commit, get_commit_message, get_pushed_to, reword_commit,
};
use gitto_git::input::{FullSha, Message, RepositoryId, Sha};
use gpui_kit::{App, AppContext as _, Context, Entity, Task};

use super::call::{call, loads, parse};
use super::client::{QueryClient, QueryError, QueryOptions, StaleTime};
use super::keys;
use super::mutation::Mutation;
use super::query::Query;

/// A commit to make: of what's staged, with `message`; with `amend`, HEAD's SHA, it replaces that
/// commit instead.
#[derive(Debug, Clone, PartialEq)]
pub struct NewCommit {
    pub message: String,
    pub amend: Option<String>,
}

/// A rewording of HEAD's commit, `sha`.
#[derive(Debug, Clone, PartialEq)]
pub struct Reword {
    pub sha: String,
    pub message: String,
}

/// Commits what's staged in the repository it's given (see [`NewCommit`]; `useCreateCommit`).
pub fn create_commit_mutation(cx: &mut App) -> Entity<Mutation<(RepositoryId, NewCommit)>> {
    Mutation::new(
        |(id, commit): (RepositoryId, NewCommit), cx: &mut App| {
            let action = call(cx, "commit.create", &id, move |repo| async move {
                let message = parse::<Message>(commit.message)?;
                let amend = commit.amend.map(parse::<FullSha>).transpose()?;
                create_commit(&repo, &message, amend.as_ref()).await
            });
            refresh_after_commit(id, action, cx)
        },
        cx,
    )
}

/// Rewrites the message of HEAD's commit, leaving what's staged out of it; resolves to the
/// rewritten commit's SHA once the repository has reloaded (`useRewordCommit`).
pub fn reword_commit_mutation(cx: &mut App) -> Entity<Mutation<(RepositoryId, Reword), String>> {
    Mutation::new(
        |(id, reword): (RepositoryId, Reword), cx: &mut App| {
            let action = call(cx, "commit.reword", &id, move |repo| async move {
                let sha = parse::<FullSha>(reword.sha)?;
                let message = parse::<Message>(reword.message)?;
                reword_commit(&repo, &sha, &message).await
            });
            refresh_after_commit(id, action, cx)
        },
        cx,
    )
}

/// Runs a commit's `action`, then reloads what it changed: the whole repository once it's done,
/// waited for; after a failure, the uncommitted changes only (`refreshAfterCommit`).
fn refresh_after_commit<R: Send + 'static>(
    id: RepositoryId,
    action: impl Future<Output = Result<R, ApiError>> + Send + 'static,
    cx: &mut App,
) -> Task<Result<R, QueryError>> {
    cx.spawn(async move |cx| {
        let result = cx.background_spawn(action).await;
        match result {
            Ok(result) => {
                cx.update(|cx| QueryClient::invalidate(&keys::repository(&id), cx))
                    .await;
                Ok(result)
            }
            Err(error) => {
                // A failure saying HEAD has moved means the status is behind; reloaded without
                // waiting for it, so the error shows right away.
                cx.update(|cx| {
                    let _ = QueryClient::invalidate(&keys::uncommitted(&id), cx);
                });
                Err(error.into())
            }
        }
    })
}

/// A commit's full message, as written; it never changes.
pub fn commit_message_options(id: &RepositoryId, sha: &str, cx: &App) -> QueryOptions<String> {
    let sha = sha.to_owned();
    QueryOptions::new(
        keys::commit_message(id, &sha),
        loads(cx, "commit.message", id, move |repo| {
            let sha = sha.clone();
            async move { get_commit_message(&repo, &parse::<Sha>(sha)?).await }
        }),
    )
    .stale_time(StaleTime::Infinite)
}

/// A commit's message's query.
pub fn commit_message_query<V: 'static>(
    id: &RepositoryId,
    sha: &str,
    cx: &mut Context<V>,
) -> Query<String> {
    Query::new(commit_message_options(id, sha, cx), cx)
}

/// The remote branch `git push` would update that has the commit; `None` if it has no need to.
pub fn pushed_to_options(id: &RepositoryId, sha: &str, cx: &App) -> QueryOptions<Option<String>> {
    let sha = sha.to_owned();
    QueryOptions::new(
        keys::pushed_to(id, &sha),
        loads(cx, "commit.pushedTo", id, move |repo| {
            let sha = sha.clone();
            async move { get_pushed_to(&repo, &parse::<Sha>(sha)?).await }
        }),
    )
    // Reloaded when the refs change (it's under `keys::repository`), not on every window focus.
    .stale_time(StaleTime::Infinite)
}

/// Where a commit was pushed to's query.
pub fn pushed_to_query<V: 'static>(
    id: &RepositoryId,
    sha: &str,
    cx: &mut Context<V>,
) -> Query<Option<String>> {
    Query::new(pushed_to_options(id, sha, cx), cx)
}
