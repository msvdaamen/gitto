//! What the UI calls git through. Ported from what the Electron app's oRPC layer did around the
//! features: `packages/git/src/core/middleware.ts` (`withRepo`), `router.ts`, `contract.ts` and
//! each feature's `contract.ts` and `handler.ts`, and the error handling of
//! `packages/rpc/src/server.ts`.
//!
//! The UI holds one [`GitApi`]. It opens a repository by its id with [`GitApi::open`], which
//! first requires a git Gitto supports, as `withRepo` did, and then calls the features with the
//! `Repo` directly: there's no method per procedure. A contract's input checks are the types the
//! features take (see [`crate::input`]), and most handlers only passed the input on. What the
//! others did is here: [`pull`] picks the branch to pull, the watches open the repository and
//! stream what changed ([`GitApi::watch_git_dir`], [`GitApi::watch_working_tree`]), and
//! `status.get` is `get_status_since`. `version.check` is `features::version::check` with
//! [`GitApi::version`]; `user.name` is `features::user::commands::get_user_name`, which needs
//! neither a repository nor a supported git.
//!
//! An error reaches the UI as an [`ApiError`], with the code, the message and whether it's a bug,
//! as the renderer got it. [`GitApi::call`] does it all: opens the repository, runs what's asked
//! with it, and turns a failure into an `ApiError`, logged, and reported if it's a bug. A call is
//! cancelled by dropping its future, which isn't a failure, as an aborted request wasn't.

mod error;
mod handlers;
#[cfg(test)]
mod tests;

use std::fmt::Write as _;
use std::future::{self, Future};
use std::sync::Arc;
use std::time::Instant;

use futures::stream::{self, BoxStream, StreamExt};

pub use self::error::ApiError;
pub use self::handlers::pull;
use crate::core::errors::{Error, Result};
use crate::core::repo::{GitRepos, Repo, RepositoryLookup};
use crate::core::trace::{trace, tracing};
use crate::core::version::GitVersion;
use crate::features::watch::commands::{watch_git_dir, watch_working_tree};
use crate::features::watch::schema::GitDirChange;
use crate::input::RepositoryId;

/// Told of each error that's a bug, with the procedure it failed (see [`GitApi::on_bug`]).
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

    /// Has `report` told of each error that's a bug ([`ApiError::is_bug`]), with the procedure it
    /// failed, e.g. to send it to a crash reporter (TS's `reportError`). It's logged already.
    pub fn on_bug(mut self, report: impl Fn(&ApiError, &str) + Send + Sync + 'static) -> Self {
        self.report_bug = Some(Arc::new(report));
        self
    }

    pub fn repos(&self) -> &GitRepos {
        &self.repos
    }

    /// The installed git, which `features::version::check` says is supported or not.
    pub fn version(&self) -> &GitVersion {
        &self.version
    }

    /// The repository with `id`, once the installed git is known to be one Gitto supports (TS's
    /// `withRepo`): nothing runs with one it doesn't, as its commands would fail partway, or be
    /// misread, rather than say why. Fails with `Error::UnsupportedGit`, or as `GitRepos::open`
    /// does: `Error::RepositoryNotFound`, `Error::FolderNotFound`, `Error::RepositoryList`.
    pub fn open(&self, id: &RepositoryId) -> impl Future<Output = Result<Repo>> + Send + use<> {
        let repos = self.repos.clone();
        let version = Arc::clone(&self.version);
        let id = id.clone();
        async move {
            version.require().await?;
            repos.open(&id).await
        }
    }

    /// Runs `task` with the repository with `id` (see [`open`](Self::open)), as the procedure
    /// `procedure` (e.g. `branch.merge`, as the renderer called it): what it resolves to, or why
    /// it failed as the UI shows it, logged and, if it's a bug, reported. `task` isn't run if the
    /// repository can't be opened. Logs how long it took when tracing (`GITTO_TRACE=1`).
    pub fn call<T, F, Fut>(
        &self,
        procedure: &'static str,
        id: &RepositoryId,
        task: F,
    ) -> impl Future<Output = Result<T, ApiError>> + 'static + use<T, F, Fut>
    where
        F: FnOnce(Repo) -> Fut + 'static,
        Fut: Future<Output = Result<T>> + 'static,
        T: 'static,
    {
        let api = self.clone();
        let open = self.open(id);
        async move {
            let start = Instant::now();
            let result = match open.await {
                Ok(repo) => task(repo).await,
                Err(error) => Err(error),
            };
            if tracing() {
                let ms = start.elapsed().as_millis();
                trace(format_args!("{ms:>5}ms {procedure}"));
            }
            result.map_err(|error| api.fail(procedure, error))
        }
    }

    /// `error` as the UI shows it, having failed `procedure`: logged (with `log`, as an error if
    /// it's a bug and a warning otherwise), and reported if it's a bug (`onError` in `server.ts`).
    /// For an error that didn't come through [`call`](Self::call).
    pub fn fail(&self, procedure: &str, error: Error) -> ApiError {
        let error = ApiError::new(error);
        if error.is_bug() {
            log::error!("{}", log_line(procedure, &error));
            if let Some(report) = &self.report_bug {
                report(&error, procedure);
            }
        } else {
            log::warn!("{}", log_line(procedure, &error));
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
        self.watch("watch.gitDir", id, |repo| watch_git_dir(&repo).boxed())
    }

    /// An item whenever a file in the working tree of the repository with `id` changes
    /// (`watch.workingTree`). It watches the whole tree (but what git ignores), so only keep it
    /// while someone's looking: it watches until it's dropped. If the repository can't be opened,
    /// that's its only item.
    pub fn watch_working_tree(
        &self,
        id: &RepositoryId,
    ) -> BoxStream<'static, Result<(), ApiError>> {
        self.watch("watch.workingTree", id, |repo| {
            watch_working_tree(&repo).boxed()
        })
    }

    /// The stream `start` watches the repository with `id` with once it's open, its errors as the
    /// UI shows them (TS's event iterators, which `withRepo` opened the repository for).
    fn watch<T: Send + 'static>(
        &self,
        procedure: &'static str,
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
        .map(move |item| item.map_err(|error| api.fail(procedure, error)))
        .boxed()
    }
}

/// How a failure is logged: the procedure, the code, and the error itself, with the command that
/// failed if it's git's.
fn log_line(procedure: &str, error: &ApiError) -> String {
    let mut line = format!(
        "[git] {procedure} failed ({}): {}",
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
