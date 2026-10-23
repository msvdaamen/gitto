//! The branches, tags and remote branches. Ported from `git/queries/refs.ts`.

use std::sync::Arc;

use gitto_git::features::refs::commands::list_refs;
use gitto_git::features::refs::schema::Ref;
use gitto_git::input::RepositoryId;
use gpui_kit::{App, Context};

use super::call::loads;
use super::client::QueryOptions;
use super::keys;
use super::query::Query;

/// The branches, tags and remote branches, shared: a reload equal to what was there keeps it, so
/// nothing shown of it renders again.
pub fn refs_options(id: &RepositoryId, cx: &App) -> QueryOptions<Arc<[Ref]>> {
    QueryOptions::new(
        keys::refs(id),
        loads(cx, "refs.list", id, |repo| async move {
            Ok(list_refs(&repo).await?.into())
        }),
    )
}

/// The refs' query (`useRefs`).
pub fn refs_query<V: 'static>(id: &RepositoryId, cx: &mut Context<V>) -> Query<Arc<[Ref]>> {
    Query::new(refs_options(id, cx), cx)
}
