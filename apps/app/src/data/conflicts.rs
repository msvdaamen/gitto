//! A conflicted file, and resolving it whole. Ported from `git/queries/conflicts.ts`.

use std::sync::Arc;
use std::time::Duration;

use gitto_git::features::conflicts::commands::{
    ShownConflict, get_conflict, keep_side, mark_resolved,
};
use gitto_git::features::conflicts::schema::{Conflict, ConflictSides, Side};
use gitto_git::input::{NonEmpty, RepositoryId};
use gpui_kit::{App, AppContext as _, Context, Entity};

use super::call::{call, loads, parse};
use super::client::{QueryClient, QueryOptions, StaleTime};
use super::keys;
use super::mutation::Mutation;
use super::query::Query;

/// How long a conflicted file is kept once nothing shows it, rather than queries' five minutes.
pub const CONFLICT_GC: Duration = Duration::from_secs(60);

/// Loads a conflicted file's sides, and its text with the conflict markers if it's text
/// (`conflictQuery`).
pub fn conflict_options(id: &RepositoryId, path: &str, cx: &App) -> QueryOptions<Conflict> {
    let path = path.to_owned();
    QueryOptions::new(
        keys::conflict(id, &path),
        loads(cx, "conflicts.get", id, move |repo| {
            let path = path.clone();
            async move { get_conflict(&repo, &parse::<NonEmpty>(path)?).await }
        }),
    )
    // Only reloaded once it's invalidated: like the status, whenever the working tree or the
    // index changes (see `file_diff::file_patch_options`).
    .stale_time(StaleTime::Infinite)
    // Not kept long once nothing shows it, as a blob isn't (see `diff::fetch_blob`): it's the whole
    // file, and one is loaded ahead whenever the pointer rests on a conflicted file.
    .gc_time(CONFLICT_GC)
}

/// [`conflict_options`] as [`conflict_query`] shows them, for `Query::set_options` when the file
/// on show changes.
pub fn conflict_query_options(
    id: &RepositoryId,
    path: &str,
    enabled: bool,
    cx: &App,
) -> QueryOptions<Conflict> {
    conflict_options(id, path, cx)
        .keep_previous_data()
        .enabled(enabled)
}

/// A conflicted file (see [`conflict_options`]), kept up to date as it's resolved (`useConflict`).
/// While another one's loads (see `Query::set_options` and [`conflict_query_options`]), the last
/// one's stays, as `Query::is_placeholder_data` says: what's on show stays until it's in. Shared
/// as it's loaded, so the view reads the whole file without copying it.
pub fn conflict_query<V: 'static>(
    id: &RepositoryId,
    path: &str,
    enabled: bool,
    cx: &mut Context<V>,
) -> Query<Conflict, Arc<Conflict>> {
    Query::with_select(
        conflict_query_options(id, path, enabled, cx),
        |conflict: &Conflict| Arc::new(conflict.clone()),
        cx,
    )
}

/// Resolving a conflicted file whole: keeping one side, or marking it resolved as it is.
#[derive(Debug, Clone, PartialEq)]
pub enum ConflictResolution {
    Keep {
        side: Side,
        /// The sides the user saw, which have to be the file's still.
        sides: ConflictSides,
        version: Option<String>,
    },
    Mark {
        version: Option<String>,
        /// Even with conflict markers left: the user said they belong in it.
        with_markers: bool,
    },
}

/// A conflicted file to resolve whole, and how.
#[derive(Debug, Clone, PartialEq)]
pub struct FileToResolve {
    pub repository_id: RepositoryId,
    pub path: String,
    pub resolution: ConflictResolution,
}

/// Resolves a conflicted file whole (see [`ConflictResolution`]; `useResolveFile`). Settles once
/// that's done, rather than once the uncommitted changes are reloaded after, so the view can move
/// on at once.
pub fn resolve_file_mutation(cx: &mut App) -> Entity<Mutation<FileToResolve>> {
    Mutation::new(
        |file: FileToResolve, cx: &mut App| {
            let FileToResolve {
                repository_id: id,
                path,
                resolution,
            } = file;
            let procedure = match resolution {
                ConflictResolution::Keep { .. } => "conflicts.keep",
                ConflictResolution::Mark { .. } => "conflicts.markResolved",
            };
            let action = call(cx, procedure, &id, move |repo| async move {
                let path = parse::<NonEmpty>(path)?;
                match resolution {
                    ConflictResolution::Keep {
                        side,
                        sides,
                        version,
                    } => keep_side(&repo, &path, side, &ShownConflict { sides, version }).await,
                    ConflictResolution::Mark {
                        version,
                        with_markers,
                    } => mark_resolved(&repo, &path, version.as_deref(), with_markers).await,
                }
            });
            cx.spawn(async move |cx| {
                let result = cx.background_spawn(action).await;
                cx.update(|cx| {
                    let _ = QueryClient::invalidate(&keys::uncommitted(&id), cx);
                });
                result.map_err(Into::into)
            })
        },
        cx,
    )
}
