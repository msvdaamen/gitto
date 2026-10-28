//! The repositories added to Gitto: listing, adding, removing and reordering them. Ported from
//! `hooks/repositories.ts`, on `RepositoryService` rather than RPC, through
//! [`RepositorySource`] (`Services::repository_source`), which tests make up.

use std::path::PathBuf;

use futures::FutureExt as _;
use futures::future::BoxFuture;
use gitto_git::input::RepositoryId;
use gitto_repository::{Repository, RepositoryService};
use gpui_kit::{App, AppContext as _, Context, Entity, Task};

use super::client::{QueryClient, QueryError, QueryOptions};
use super::keys;
use super::query::Query;
use crate::services::{Services, SystemContext};
use crate::state::router::{Route, Router};

/// Where the list of repositories is kept: `RepositoryService` (the database), or a test's.
pub trait RepositorySource {
    /// In the order of their tabs.
    fn list(&self) -> BoxFuture<'static, Result<Vec<Repository>, QueryError>>;
    /// Adds the git repository at `path` (see `RepositoryService::add_repository`).
    fn add(&self, path: PathBuf) -> BoxFuture<'static, Result<Repository, QueryError>>;
    fn remove(&self, id: RepositoryId) -> BoxFuture<'static, Result<(), QueryError>>;
    /// Puts the tabs in the order of `ids`.
    fn reorder(&self, ids: Vec<RepositoryId>) -> BoxFuture<'static, Result<(), QueryError>>;
}

impl RepositorySource for RepositoryService {
    fn list(&self) -> BoxFuture<'static, Result<Vec<Repository>, QueryError>> {
        let service = self.clone();
        async move { Ok(service.get_repositories().await?) }.boxed()
    }

    fn add(&self, path: PathBuf) -> BoxFuture<'static, Result<Repository, QueryError>> {
        let service = self.clone();
        async move { Ok(service.add_repository(path).await?) }.boxed()
    }

    fn remove(&self, id: RepositoryId) -> BoxFuture<'static, Result<(), QueryError>> {
        let service = self.clone();
        async move { Ok(service.remove_repository(&id).await?) }.boxed()
    }

    fn reorder(&self, ids: Vec<RepositoryId>) -> BoxFuture<'static, Result<(), QueryError>> {
        let service = self.clone();
        async move { Ok(service.reorder_repositories(&ids).await?) }.boxed()
    }
}

/// The repositories added to Gitto (name and path), in the order of their tabs. A reload equal to
/// what was there keeps it, so nothing shown of it renders again (TS's `reconcile: "id"`).
pub fn repositories_options(cx: &App) -> QueryOptions<Vec<Repository>> {
    let source = Services::global(cx).repository_source().clone();
    QueryOptions::new(keys::repositories(), move |_, _| source.list())
}

/// The repositories' query (`useRepositories`).
pub fn repositories_query<V: 'static>(cx: &mut Context<V>) -> Query<Vec<Repository>> {
    Query::new(repositories_options(cx), cx)
}

/// The repository with `id` among `repositories` (`useRepository`).
pub fn find<'a>(repositories: &'a [Repository], id: &RepositoryId) -> Option<&'a Repository> {
    repositories.iter().find(|repository| repository.id == *id)
}

/// Asks for a folder and adds the repository in it; resolves to the repository, or `None` when the
/// folder picker was cancelled (`useAddRepository`). The list is loaded again once it's added.
pub fn add_repository(cx: &mut App) -> Task<Result<Option<Repository>, QueryError>> {
    let source = Services::global(cx).repository_source().clone();
    let folder = SystemContext::select_folder(cx);
    cx.spawn(async move |cx| {
        let result = async {
            let Some(path) = folder.await? else {
                return Ok(None);
            };
            Ok::<_, QueryError>(Some(source.add(path).await?))
        }
        .await;
        match &result {
            Ok(_) => {
                cx.update(|cx| QueryClient::invalidate(&keys::repositories(), cx))
                    .await;
            }
            // The UI shows only the message; the rest is for whoever looks into it.
            Err(error) => log::error!("Couldn't add the repository: {error:?}"),
        }
        result
    })
}

/// Removes a repository from Gitto; the folder on disk is left untouched (`useRemoveRepository`).
pub fn remove_repository(id: RepositoryId, cx: &mut App) -> Task<Result<(), QueryError>> {
    let removing = Services::global(cx).repository_source().remove(id);
    cx.spawn(async move |cx| {
        let result = removing.await;
        match &result {
            Ok(()) => {
                cx.update(|cx| QueryClient::invalidate(&keys::repositories(), cx))
                    .await;
            }
            Err(error) => log::error!("Couldn't remove the repository: {error:?}"),
        }
        result
    })
}

/// Puts the repositories' tabs in the order of `ids`; the list is loaded again whether that went
/// through or not (`useReorderRepositories`).
pub fn reorder_repositories(ids: Vec<RepositoryId>, cx: &mut App) -> Task<Result<(), QueryError>> {
    let reordering = Services::global(cx).repository_source().reorder(ids);
    cx.spawn(async move |cx| {
        let result = reordering.await;
        if let Err(error) = &result {
            log::error!("Couldn't reorder the repositories: {error:?}");
        }
        cx.update(|cx| QueryClient::invalidate(&keys::repositories(), cx))
            .await;
        result
    })
}

/// Asks for a folder, adds the repository in it (see [`add_repository`]), and opens it; with why
/// that last failed (the folder isn't a repository, say), until it's dismissed or tried again
/// (`useOpenRepository`). Each place that offers it keeps its own: the tabs' `+`, the home page's
/// button.
#[derive(Default)]
pub struct OpenRepository {
    error: Option<QueryError>,
    task: Option<Task<()>>,
}

impl OpenRepository {
    pub fn new(cx: &mut App) -> Entity<OpenRepository> {
        cx.new(|_| OpenRepository::default())
    }

    /// Why it last failed, until it's dismissed or tried again.
    pub fn error(&self) -> Option<&QueryError> {
        self.error.as_ref()
    }

    pub fn open(&mut self, cx: &mut Context<Self>) {
        let adding = add_repository(cx);
        self.task = Some(cx.spawn(async move |this, cx| {
            let result = adding.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(Some(repository)) => {
                        this.error = None;
                        Router::navigate(Route::Repository(repository.id), cx);
                    }
                    Ok(None) => this.error = None,
                    Err(error) => this.error = Some(error),
                }
                this.task = None;
                cx.notify();
            });
        }));
    }

    /// Forgets why it last failed.
    pub fn dismiss(&mut self, cx: &mut Context<Self>) {
        if self.error.take().is_some() {
            cx.notify();
        }
    }
}
