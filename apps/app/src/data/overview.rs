//! What the home page shows of a repository. Ported from `git/queries/overview.ts`.

use futures::FutureExt as _;
use gitto_git::features::overview::schema::Overview;
use gitto_git::input::RepositoryId;
use gpui_kit::{App, Context};

use super::client::QueryOptions;
use super::keys;
use super::query::Query;
use super::watch::unwatched;
use crate::services::Services;

/// What the home page shows of a repository, beyond its status: where it's hosted, when it was
/// last fetched and committed to, and what was last done in it. Not watched (see `unwatched`).
pub fn overview_options(id: &RepositoryId, cx: &App) -> QueryOptions<Overview> {
    let git = Services::global(cx).git().clone();
    let id = id.clone();
    unwatched(QueryOptions::new(keys::overview(&id), move |_, _| {
        let open = git.repo(&id);
        async move { open.await?.overview().await }
            .map(|result| result.map_err(Into::into))
            .boxed()
    }))
}

/// A repository's overview's query (`useOverview`).
pub fn overview_query<V: 'static>(id: &RepositoryId, cx: &mut Context<V>) -> Query<Overview> {
    Query::new(overview_options(id, cx), cx)
}
