//! The worktrees: listing, adding, removing and opening them. Ported from
//! `git/queries/worktree.ts`.

use std::path::PathBuf;
use std::sync::Arc;

use gitto_git::features::worktree::commands::{add_worktree, list_worktrees, remove_worktree};
use gitto_git::features::worktree::schema::Worktree;
use gitto_git::input::{BranchName, BranchRef, Folder, RepositoryId};
use gitto_repository::Repository;
use gpui_kit::{App, AppContext as _, Context};

use super::branch::{ADD_WORKTREE, branches_changing};
use super::call::{call, loads, parse};
use super::client::{QueryClient, QueryError, QueryOptions};
use super::keys;
use super::operation::{OneAtATime, Operations, RepositoryOperation, reloading};
use super::query::Query;
use crate::services::Services;
use crate::state::router::{Route, Router};

/// The worktrees, the main one first; the one on show is marked `current`. A reload equal to what
/// was there keeps it, so one that brings nothing new (the refs changed, say) doesn't lay out and
/// render the history again (TS's `reconcile: "path"`).
pub fn worktrees_options(id: &RepositoryId, cx: &App) -> QueryOptions<Arc<[Worktree]>> {
    QueryOptions::new(
        keys::worktrees(id),
        loads(cx, "worktree.list", id, |repo| async move {
            Ok(list_worktrees(&repo).await?.into())
        }),
    )
}

/// The worktrees' query (`useWorktrees`).
pub fn worktrees_query<V: 'static>(
    id: &RepositoryId,
    cx: &mut Context<V>,
) -> Query<Arc<[Worktree]>> {
    Query::new(worktrees_options(id, cx), cx)
}

/// What a worktree is added with: the folder, the branch (by its full ref name), and a new
/// branch's name, if one is made from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddWorktreeInput {
    pub path: String,
    pub branch: String,
    pub new_branch: Option<String>,
}

const OPEN: &str = "open-worktree";
const REMOVE: &str = "remove-worktree";

/// Opens a worktree of the repository, by its path: adds it to Gitto's repositories, as one of its
/// own, or finds it there, and shows it (`useOpenWorktree`). Whether that's running, and why it
/// last failed, wherever it was started from (a worktree's menu, or a branch's).
pub fn open_worktree_operation() -> RepositoryOperation<String> {
    RepositoryOperation::new(OPEN, |_id, path: String, cx: &mut App| {
        let adding = Services::global(cx)
            .repository_source()
            .add(PathBuf::from(path));
        cx.spawn(async move |cx| {
            let repository = adding.await?;
            cx.update(|cx| QueryClient::invalidate(&keys::repositories(), cx))
                .await;
            let id = RepositoryId::parse(repository.id)?;
            cx.update(|cx| Router::navigate(Route::Repository(id), cx));
            Ok(())
        })
    })
}

/// Adding a worktree, removing one, and opening one (`useWorktreeActions`); whether each is
/// running, and why it last failed. Adding and removing count as running until the repository has
/// reloaded, so the worktrees on show are never ones from before. One runs at a time
/// ([`is_pending`](Self::is_pending)). Adding one is also a change of the branches, as it checks
/// one out: it doesn't run while another runs (a switch, a deletion…), nor they while it does
/// (see `branch::branches_changing`); whether it can't run now is
/// [`add_is_blocked`](Self::add_is_blocked). A worktree that was opened in Gitto is removed from
/// its repositories too, as its folder is gone.
#[derive(Clone)]
pub struct WorktreeActions {
    pub add: OneAtATime<AddWorktreeInput>,
    /// Removes a worktree, by its path.
    pub remove: OneAtATime<String>,
    /// Opens a worktree, by its path (see [`open_worktree_operation`]).
    pub open: OneAtATime<String>,
}

impl WorktreeActions {
    /// Whether any of them is running on the repository `id`.
    pub fn is_pending(id: &str, cx: &App) -> bool {
        [ADD_WORKTREE, REMOVE, OPEN]
            .iter()
            .any(|name| Operations::state(name, id, cx).running)
    }

    /// Whether adding one can't run on the repository `id` now: one of them is running, or a
    /// change of the branches is.
    pub fn add_is_blocked(id: &str, cx: &App) -> bool {
        WorktreeActions::is_pending(id, cx) || branches_changing(id, cx)
    }
}

/// The operations of [`WorktreeActions`].
pub fn worktree_actions() -> WorktreeActions {
    let busy = |id: &str, cx: &App| WorktreeActions::is_pending(id, cx);
    WorktreeActions {
        add: RepositoryOperation::new(ADD_WORKTREE, |id, input: AddWorktreeInput, cx: &mut App| {
            let action = call(cx, "worktree.add", &id, move |repo| async move {
                let folder = parse::<Folder>(input.path)?;
                let branch = parse::<BranchRef>(input.branch)?;
                let new_branch = input.new_branch.map(parse::<BranchName>).transpose()?;
                add_worktree(&repo, &folder, &branch, new_branch.as_ref()).await
            });
            cx.spawn(async move |cx| reloading(&id, action, cx).await)
        })
        .one_at_a_time(WorktreeActions::add_is_blocked),
        remove: RepositoryOperation::new(REMOVE, |id, path: String, cx: &mut App| {
            let removing = {
                let path = path.clone();
                call(cx, "worktree.remove", &id, move |repo| async move {
                    remove_worktree(&repo, &path).await
                })
            };
            let source = Services::global(cx).repository_source().clone();
            cx.spawn(async move |cx| {
                // Removes the worktree, and the repository it was opened as, if it was.
                let result = async {
                    cx.background_spawn(removing).await?;
                    let cached = cx.update(|cx| {
                        QueryClient::get_data::<Vec<Repository>>(&keys::repositories(), cx)
                    });
                    let repositories = match cached {
                        Some(repositories) => repositories,
                        None => source.list().await?,
                    };
                    let Some(opened) = repositories
                        .into_iter()
                        .find(|repository| repository.path == path)
                    else {
                        return Ok(());
                    };
                    source.remove(parse(opened.id)?).await?;
                    cx.update(|cx| QueryClient::invalidate(&keys::repositories(), cx))
                        .await;
                    Ok::<_, QueryError>(())
                }
                .await;
                cx.update(|cx| QueryClient::invalidate(&keys::repository(&id), cx))
                    .await;
                result
            })
        })
        .one_at_a_time(busy),
        open: open_worktree_operation().one_at_a_time(busy),
    }
}
