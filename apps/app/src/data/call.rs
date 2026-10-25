//! Calling git from the data layer: what each query and operation module does to run a feature
//! with a repository, as `rpc.git.*` did.

use std::future::Future;

use futures::FutureExt as _;
use futures::future::BoxFuture;
use gitto_git::input::RepositoryId;
use gitto_git::{ApiError, GitApi, GitRepo};
use gpui_kit::App;

use super::client::QueryError;
use crate::services::Services;

/// The app's git (see `Services::git`).
pub(crate) fn git(cx: &App) -> GitApi {
    Services::global(cx).git().clone()
}

/// Opens the repository `id` (see `GitApi::repo`) and runs `task` with it, off the main thread
/// once it's spawned there.
pub(crate) fn call<T, F, Fut>(
    cx: &App,
    id: &RepositoryId,
    task: F,
) -> impl Future<Output = Result<T, ApiError>> + Send + 'static + use<T, F, Fut>
where
    F: FnOnce(GitRepo) -> Fut + Send + 'static,
    Fut: Future<Output = Result<T, ApiError>> + Send + 'static,
{
    let open = git(cx).repo(id);
    async move { task(open.await?).await }
}

/// A query's fetcher that opens the repository `id` and runs `task` with it each time it loads
/// (see `QueryOptions::new`).
pub(crate) fn loads<T, F, Fut>(
    cx: &App,
    id: &RepositoryId,
    task: F,
) -> impl Fn(Option<&T>, &App) -> BoxFuture<'static, Result<T, QueryError>> + 'static
where
    F: Fn(GitRepo) -> Fut + Clone + Send + 'static,
    Fut: Future<Output = Result<T, ApiError>> + Send + 'static,
    T: Send + 'static,
{
    let git = git(cx);
    let id = id.clone();
    move |_, _| {
        let open = git.repo(&id);
        let task = task.clone();
        async move {
            let repo = open.await?;
            let load = task(repo);
            Ok(load.await?)
        }
        .boxed()
    }
}
