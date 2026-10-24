//! Calling git from the data layer: what each query and operation module does to run a feature
//! with a repository, as `rpc.git.*` did.

use std::future::Future;

use futures::FutureExt as _;
use futures::future::BoxFuture;
use gitto_git::input::RepositoryId;
use gitto_git::{ApiError, GitApi, Repo};
use gpui_kit::App;

use super::client::QueryError;
use crate::services::Services;

/// The app's git (see `Services::git`).
pub(crate) fn git(cx: &App) -> GitApi {
    Services::global(cx).git().clone()
}

/// `value` as the input type `T`, checked as the procedure's contract checked it: a failure is
/// `Error::InvalidInput`, which reaches the UI as the API's "Input validation failed", as a request
/// zod refused did. The UI hands over what git gave it (a SHA, a ref), so it only fails on a bug.
pub(crate) fn parse<T>(value: impl Into<String>) -> gitto_git::Result<T>
where
    T: TryFrom<String, Error = gitto_git::Error>,
{
    T::try_from(value.into())
}

/// Runs `task` with the repository `id`, as the procedure `procedure` (see `GitApi::call`), off
/// the main thread once it's spawned there.
pub(crate) fn call<T, F, Fut>(
    cx: &App,
    procedure: &'static str,
    id: &RepositoryId,
    task: F,
) -> impl Future<Output = Result<T, ApiError>> + Send + 'static + use<T, F, Fut>
where
    F: FnOnce(Repo) -> Fut + Send + 'static,
    Fut: Future<Output = gitto_git::Result<T>> + Send + 'static,
    T: Send + 'static,
{
    git(cx).call(procedure, id, task)
}

/// A query's fetcher that runs `task` with the repository `id` as `procedure` each time it loads
/// (see `QueryOptions::new`).
pub(crate) fn loads<T, F, Fut>(
    cx: &App,
    procedure: &'static str,
    id: &RepositoryId,
    task: F,
) -> impl Fn(Option<&T>, &App) -> BoxFuture<'static, Result<T, QueryError>> + 'static
where
    F: Fn(Repo) -> Fut + Clone + Send + 'static,
    Fut: Future<Output = gitto_git::Result<T>> + Send + 'static,
    T: Send + 'static,
{
    let git = git(cx);
    let id = id.clone();
    move |_, _| {
        git.call(procedure, &id, task.clone())
            .map(|result| result.map_err(Into::into))
            .boxed()
    }
}
