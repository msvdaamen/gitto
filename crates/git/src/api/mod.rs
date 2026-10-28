//! What the UI calls git through. Ported from what the Electron app's oRPC layer did around the
//! features: `packages/git/src/core/middleware.ts` (`withRepo`), `router.ts`, `contract.ts` and
//! each feature's `contract.ts` and `handler.ts`, and the error handling of
//! `packages/rpc/src/server.ts`.
//!
//! The UI holds one [`GitApi`]. It opens a repository by its id with [`GitApi::repo`], which
//! first requires a git Gitto supports, as `withRepo` did, and gives a [`GitRepo`]: a method for
//! each thing the UI does with a repository, which runs the feature. A contract's input checks are
//! the types the methods take (see [`crate::input`]). What handlers did beyond passing the input
//! on is here: [`pull`] picks the branch to pull, the watches open the repository and stream what
//! changed ([`GitApi::watch_git_dir`], [`GitApi::watch_working_tree`]), and `status.get` is
//! `get_status_since`. `version.check` is [`GitApi::version`]'s `check`; `user.name` is
//! [`get_user_name`](crate::get_user_name), which needs neither a repository nor a supported git.
//!
//! An error reaches the UI as an [`ApiError`], with the code, the message and whether it's a bug,
//! as the renderer got it: each `GitRepo` method turns a failure into one, logged under the
//! method's name, and reported if it's a bug. A method is cancelled by dropping its future, which
//! isn't a failure, as an aborted request wasn't.

mod error;
mod handlers;
mod repo;
#[cfg(test)]
mod tests;

use std::fmt::Write as _;
use std::future::{self, Future};
use std::sync::Arc;

use futures::stream::{self, BoxStream, StreamExt};

pub use self::error::ApiError;
pub use self::handlers::pull;
pub use self::repo::GitRepo;
use crate::core::errors::{Error, Result};
use crate::core::repo::{GitRepos, Repo, RepositoryLookup};
use crate::core::version::GitVersion;
use crate::features::watch::commands::{watch_git_dir, watch_working_tree};
use crate::features::watch::schema::GitDirChange;
use crate::input::RepositoryId;

/// Told of each error that's a bug, with what failed (see [`GitApi::on_bug`]).
type ReportBug = Arc<dyn Fn(&ApiError, &str) + Send + Sync>;

/// The git backend as the UI uses it: the repositories added to Gitto, and the installed git,
/// which nothing runs without (TS's `GitContext`, with `createRpcHandler`'s `reportError`). Cheap
/// to clone; clones share it all.
#[derive(Clone)]
pub struct GitApi {
    repos: GitRepos,
    version: Arc<GitVersion>,
    report_bug: Option<ReportBug>,
}

impl GitApi {
    /// Opens the repositories `lookup` finds, with the git on the PATH.
    pub fn new(lookup: impl RepositoryLookup) -> Self {
        Self::with(GitRepos::new(lookup), GitVersion::new())
    }

    /// Opens the repositories `repos` does, with the git `version` checks.
    pub fn with(repos: GitRepos, version: GitVersion) -> Self {
        GitApi {
            repos,
            version: Arc::new(version),
            report_bug: None,
        }
    }

    /// Has `report` told of each error that's a bug ([`ApiError::is_bug`]), with what failed (a
    /// `GitRepo` method's name), e.g. to send it to a crash reporter (TS's `reportError`). It's
    /// logged already.
    pub fn on_bug(mut self, report: impl Fn(&ApiError, &str) + Send + Sync + 'static) -> Self {
        self.report_bug = Some(Arc::new(report));
        self
    }

    /// The installed git, which `features::version::check` says is supported or not.
    pub fn version(&self) -> &GitVersion {
        &self.version
    }

    /// The repository with `id`, once the installed git is known to be one Gitto supports (TS's
    /// `withRepo`): nothing runs with one it doesn't, as its commands would fail partway, or be
    /// misread, rather than say why. Fails with `Error::UnsupportedGit`, or as `GitRepos::open`
    /// does: `Error::RepositoryNotFound`, `Error::FolderNotFound`, `Error::RepositoryList`.
    pub(crate) fn open(
        &self,
        id: &RepositoryId,
    ) -> impl Future<Output = Result<Repo>> + Send + use<> {
        let repos = self.repos.clone();
        let version = Arc::clone(&self.version);
        let id = id.clone();
        async move {
            version.require().await?;
            repos.open(&id).await
        }
    }

    /// The repository with `id`, opened (see [`open`](Self::open)), with a method for each thing
    /// the UI does with it. Fails as the UI shows it, logged and, if it's a bug, reported.
    pub fn repo(
        &self,
        id: &RepositoryId,
    ) -> impl Future<Output = Result<GitRepo, ApiError>> + Send + 'static + use<> {
        let api = self.clone();
        let open = self.open(id);
        async move {
            match open.await {
                Ok(repo) => Ok(GitRepo::new(repo, api)),
                Err(error) => Err(api.fail("open", error)),
            }
        }
    }

    /// `error` as the UI shows it, `action` having failed: logged (with `log`, as an error if it's
    /// a bug and a warning otherwise), and reported if it's a bug (`onError` in `server.ts`).
    /// [`GitRepo`]'s methods fail through it.
    pub fn fail(&self, action: &str, error: Error) -> ApiError {
        let error = ApiError::new(error);
        if error.is_bug() {
            log::error!("{}", log_line(action, &error));
            if let Some(report) = &self.report_bug {
                report(&error, action);
            }
        } else {
            log::warn!("{}", log_line(action, &error));
        }
        error
    }

    /// What changes in the git directory of the repository with `id`, e.g. a commit, a checkout
    /// or staging from another tool (`watch.gitDir`): a handful of folders, so it's cheap to keep.
    /// Watches until it's dropped. If the repository can't be opened, that's its only item.
    pub fn watch_git_dir(
        &self,
        id: &RepositoryId,
    ) -> BoxStream<'static, Result<Vec<GitDirChange>, ApiError>> {
        self.watch("watch_git_dir", id, |repo| watch_git_dir(&repo).boxed())
    }

    /// An item whenever a file in the working tree of the repository with `id` changes
    /// (`watch.workingTree`). It watches the whole tree (but what git ignores), so only keep it
    /// while someone's looking: it watches until it's dropped. If the repository can't be opened,
    /// that's its only item.
    pub fn watch_working_tree(
        &self,
        id: &RepositoryId,
    ) -> BoxStream<'static, Result<(), ApiError>> {
        self.watch("watch_working_tree", id, |repo| {
            watch_working_tree(&repo).boxed()
        })
    }

    /// The stream `start` watches the repository with `id` with once it's open, its errors as the
    /// UI shows them (TS's event iterators, which `withRepo` opened the repository for).
    fn watch<T: Send + 'static>(
        &self,
        action: &'static str,
        id: &RepositoryId,
        start: impl FnOnce(Repo) -> BoxStream<'static, Result<T>> + Send + 'static,
    ) -> BoxStream<'static, Result<T, ApiError>> {
        let api = self.clone();
        let open = self.open(id);
        stream::once(async move {
            match open.await {
                Ok(repo) => start(repo),
                Err(error) => stream::once(future::ready(Err(error))).boxed(),
            }
        })
        .flatten()
        .map(move |item| item.map_err(|error| api.fail(action, error)))
        .boxed()
    }
}

/// How a failure is logged: what failed, the code, and the error itself, with the command that
/// failed if it's git's.
fn log_line(action: &str, error: &ApiError) -> String {
    let mut line = format!(
        "[git] {action} failed ({}): {}",
        error.code().code(),
        error.error()
    );
    if let Some(git) = error.error().as_git() {
        let _ = write!(line, "\n  git {}", git.args.join(" "));
        if let Some(code) = git.exit_code {
            let _ = write!(line, " (exit code {code})");
        }
        let stderr = git.stderr.trim();
        if !stderr.is_empty() && stderr != git.message {
            let _ = write!(line, "\n  {stderr}");
        }
    }
    line
}
