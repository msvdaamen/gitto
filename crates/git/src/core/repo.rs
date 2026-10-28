//! A repository on disk, with git commands bound to it, and the helpers every feature uses.
//!
//! Ported from `packages/git/src/core/repo.ts`. `GitRepos` looks repositories up through
//! [`RepositoryLookup`] in place of TS's `RepositoryService`.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::future::BoxFuture;

use super::errors::{Error, Result};
use super::runner::{
    CommitGraphs, IndexRefreshes, RunOptions, ShownStatuses, WriteQueue, lock, os_args,
    run_git_bytes,
};

/// Finds the folder of a repository that's been added to Gitto, by its id (TS's
/// `RepositoryService.getRepository`). A function `Fn(&str) -> Option<PathBuf>` is one.
pub trait RepositoryLookup: Send + Sync + 'static {
    /// The folder of the repository with `repository_id`; `None` if there's none.
    fn repository_path<'a>(&'a self, repository_id: &'a str) -> BoxFuture<'a, Option<PathBuf>>;
}

impl<F> RepositoryLookup for F
where
    F: Fn(&str) -> Option<PathBuf> + Send + Sync + 'static,
{
    fn repository_path<'a>(&'a self, repository_id: &'a str) -> BoxFuture<'a, Option<PathBuf>> {
        Box::pin(std::future::ready(self(repository_id)))
    }
}

/// What all repositories share: the queues and caches of `runner`, by repository path.
struct Shared {
    writes: Arc<WriteQueue>,
    fetches: WriteQueue,
    commit_graphs: CommitGraphs,
    shown_statuses: ShownStatuses,
    index_refreshes: Arc<IndexRefreshes>,
}

/// Opens the repositories that have been added to Gitto. Cheap to clone; clones share their state.
#[derive(Clone)]
pub struct GitRepos {
    inner: Arc<GitReposInner>,
}

struct GitReposInner {
    lookup: Box<dyn RepositoryLookup>,
    shared: Arc<Shared>,
    /// Each repository, by its path: the same path is the same repository, whatever its id.
    repos: Mutex<HashMap<PathBuf, Repo>>,
}

impl GitRepos {
    pub fn new(lookup: impl RepositoryLookup) -> Self {
        let writes = Arc::new(WriteQueue::new());
        let shared = Arc::new(Shared {
            index_refreshes: Arc::new(IndexRefreshes::new(Arc::clone(&writes))),
            writes,
            fetches: WriteQueue::new(),
            commit_graphs: CommitGraphs::default(),
            shown_statuses: ShownStatuses::default(),
        });
        GitRepos {
            inner: Arc::new(GitReposInner {
                lookup: Box::new(lookup),
                shared,
                repos: Mutex::new(HashMap::new()),
            }),
        }
    }

    /// The repository, looked up by its id and checked to be a folder every time: one that's
    /// removed from Gitto, or whose folder is gone, is refused with why
    /// (`Error::RepositoryNotFound`, `Error::FolderNotFound`). What's kept for it is made once.
    pub async fn open(&self, repository_id: &str) -> Result<Repo> {
        let Some(path) = self.inner.lookup.repository_path(repository_id).await else {
            return Err(Error::RepositoryNotFound {
                repository_id: repository_id.to_owned(),
            });
        };
        // Checked up front: git can't start in a missing folder, and spawning it reports that as
        // it does a missing git binary.
        let is_folder = async_fs::metadata(&path)
            .await
            .is_ok_and(|metadata| metadata.is_dir());
        if !is_folder {
            return Err(Error::FolderNotFound {
                path: path.display().to_string(),
            });
        }
        let mut repos = lock(&self.inner.repos);
        let repo = repos.entry(path.clone()).or_insert_with(|| Repo {
            inner: Arc::new(RepoInner {
                path,
                shared: Arc::clone(&self.inner.shared),
                state: Arc::new(RepoState {
                    sorts_log: AtomicBool::new(false),
                    kept: Mutex::new(HashMap::new()),
                }),
                #[cfg(test)]
                observer: None,
            }),
        });
        Ok(repo.clone())
    }
}

/// A repository on disk, with git commands bound to it. Cheap to clone; clones are the same
/// repository.
#[derive(Clone)]
pub struct Repo {
    inner: Arc<RepoInner>,
}

struct RepoInner {
    path: PathBuf,
    shared: Arc<Shared>,
    state: Arc<RepoState>,
    /// Told what the repository does, in tests (see `Repo::observed`).
    #[cfg(test)]
    observer: Option<Observer>,
}

/// What's kept for one repository while the app runs.
struct RepoState {
    /// Whether git has to sort its log (see `Repo::sorts_log`).
    sorts_log: AtomicBool,
    /// What the features keep for it (see `Repo::keep`).
    kept: Mutex<HashMap<TypeId, Arc<dyn Any + Send + Sync>>>,
}

#[cfg(test)]
type Observer = Arc<dyn Fn(&Observed) + Send + Sync>;

/// What a test is told a repository from `Repo::observed` did.
#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Observed {
    /// A command that's done (succeeded, failed or was dropped), how it ran, and its arguments,
    /// lossily UTF-8.
    Ran { how: How, args: Vec<String> },
    /// `Repo::status_took` was called with this.
    StatusTook(Duration),
}

/// How an `Observed` command ran: through which of `Repo`'s commands.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum How {
    /// `read`, or `git()`.
    Read,
    /// `write`, or `writer()`.
    Write,
    /// `fetch`, or `fetcher()`.
    Fetch,
    /// The command `exclusive` gives its task.
    Exclusive,
    /// The command `fetching` gives its task.
    Fetching,
}

impl Repo {
    pub fn path(&self) -> &Path {
        &self.inner.path
    }

    /// Runs commands that don't change the repository, straight away (TS's `repo.read`, and the
    /// `run` that `exclusive` gives its task).
    pub fn git(&self) -> GitCommand {
        self.command(Via::Now)
    }

    /// Runs each command as a write of its own (TS's `repo.write`, as a `GitCommand`).
    pub fn writer(&self) -> GitCommand {
        self.command(Via::Writes)
    }

    /// Runs each command as a fetch of its own (TS's `repo.fetch`, as a `GitCommand`).
    pub fn fetcher(&self) -> GitCommand {
        self.command(Via::Fetches)
    }

    fn command(&self, via: Via) -> GitCommand {
        GitCommand {
            repo: Arc::clone(&self.inner),
            via,
        }
    }

    /// Runs a command that doesn't change the repository.
    pub fn read<I, S>(&self, args: I) -> BoxFuture<'static, Result<String>>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.git().run(args)
    }

    /// `read`, with `options`.
    pub fn read_with<I, S>(
        &self,
        args: I,
        options: RunOptions,
    ) -> BoxFuture<'static, Result<String>>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.git().run_with(args, options)
    }

    /// Runs a command that changes the repository, after any earlier writes to it have finished:
    /// ones whose future was polled before this one's (see `GitCommand`).
    pub fn write<I, S>(&self, args: I) -> BoxFuture<'static, Result<String>>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.writer().run(args)
    }

    /// `write`, with `options`.
    pub fn write_with<I, S>(
        &self,
        args: I,
        options: RunOptions,
    ) -> BoxFuture<'static, Result<String>>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.writer().run_with(args, options)
    }

    /// Runs a fetch. Not queued with writes: it waits on the network, which a commit or staging a
    /// file shouldn't wait for, and it only updates remote-tracking refs and FETCH_HEAD, which they
    /// don't touch. Fetches of the same repository do run one at a time, and are stopped if Gitto
    /// exits.
    pub fn fetch<I, S>(&self, args: I) -> BoxFuture<'static, Result<String>>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.fetcher().run(args)
    }

    /// `fetch`, with `options`.
    pub fn fetch_with<I, S>(
        &self,
        args: I,
        options: RunOptions,
    ) -> BoxFuture<'static, Result<String>>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.fetcher().run_with(args, options)
    }

    /// Runs `task`, which runs commands through the `GitCommand` it's given, as one fetch (see
    /// `fetch`): no other fetch of the repository runs before it's done, e.g. to read the
    /// FETCH_HEAD a fetch wrote. Its place in line is taken when the future is first polled.
    pub fn fetching<T, F, Fut>(&self, task: F) -> impl Future<Output = T> + use<T, F, Fut>
    where
        F: FnOnce(GitCommand) -> Fut,
        Fut: Future<Output = T>,
    {
        let turn = self.inner.shared.fetches.turn(self.inner.path.clone());
        let run = self.command(Via::Fetching);
        async move {
            let _turn = turn.await;
            task(run).await
        }
    }

    /// Runs `task`, which runs commands through the `GitCommand` it's given, as one write: no
    /// other write to the repository runs before it's done. For a write that checks the repository
    /// before and after. Its place in line is taken when the future is first polled. Don't use
    /// `writer()`/`write` inside it, nor a feature that does: that waits for `task` to end, which
    /// never comes (it's logged as a warning after a while, see `WriteQueue`). Run its commands
    /// through the `GitCommand` it's given.
    pub fn exclusive<T, F, Fut>(&self, task: F) -> impl Future<Output = T> + use<T, F, Fut>
    where
        F: FnOnce(GitCommand) -> Fut,
        Fut: Future<Output = T>,
    {
        let turn = self.inner.shared.writes.turn(self.inner.path.clone());
        let run = self.command(Via::Exclusive);
        async move {
            let _turn = turn.await;
            task(run).await
        }
    }

    /// Told how long reading the status took: the index is refreshed if the first one of this run
    /// was slow, as after a fresh clone (see `IndexRefreshes`).
    pub fn status_took(&self, took: Duration) {
        #[cfg(test)]
        if let Some(observer) = &self.inner.observer {
            observer(&Observed::StatusTook(took));
        }
        self.inner
            .shared
            .index_refreshes
            .status_took(&self.inner.path, took);
    }

    /// Whether the log has to be sorted by git: an earlier read, left unsorted, had a commit below
    /// one of its parents (see `getLog`). Remembered while the app runs, once
    /// `mark_log_needs_sorting` is called.
    pub fn sorts_log(&self) -> bool {
        self.inner.state.sorts_log.load(Ordering::Relaxed)
    }

    /// Has git sort the log from now on (see `sorts_log`).
    pub fn mark_log_needs_sorting(&self) {
        self.inner.state.sorts_log.store(true, Ordering::Relaxed);
    }

    /// Whether HEAD points at a commit; it doesn't on a branch without commits yet.
    pub fn has_head(&self) -> impl Future<Output = bool> + Send + 'static + use<> {
        let run = self.git();
        // Any failure is taken for no HEAD, as callers have always had it.
        async move { ref_exists(&run, "HEAD").await.unwrap_or(false) }
    }

    /// Writes the commit-graph, which speeds up sorting the log, once per run (`CommitGraphs`).
    /// It starts now, and runs to its end even if the future is dropped.
    pub fn update_commit_graph(&self) -> impl Future<Output = ()> + Send + 'static + use<> {
        self.inner.shared.commit_graphs.update(&self.inner.path)
    }

    /// Remembers the status the UI is being sent, as its `statusSnapshot`: `snapshot` resolves to
    /// `None` if the UI didn't get it after all (see `ShownStatuses`).
    pub fn show_status(&self, snapshot: impl Future<Output = Option<String>> + Send + 'static) {
        self.inner
            .shared
            .shown_statuses
            .set(&self.inner.path, snapshot);
    }

    /// The snapshot of the status the UI last got, once any that's being read is; `None` if it
    /// didn't get one.
    pub fn shown_status(&self) -> impl Future<Output = Option<String>> + Send + 'static + use<> {
        let inner = Arc::clone(&self.inner);
        async move { inner.shared.shown_statuses.get(&inner.path).await }
    }

    /// What a feature keeps for the repository while the app runs, by its type: made (with
    /// `Default`) the first time it's asked for, the same thing every time after. E.g. the versions
    /// of the files it saved. Give each thing kept a type of its own (a private newtype), so no two
    /// features share one by accident; it needs its own locking, e.g. a `Mutex` inside.
    pub fn keep<T: Default + Send + Sync + 'static>(&self) -> Arc<T> {
        let kept = Arc::clone(
            lock(&self.inner.state.kept)
                .entry(TypeId::of::<T>())
                .or_insert_with(|| Arc::new(T::default())),
        );
        kept.downcast::<T>()
            .unwrap_or_else(|_| unreachable!("kept by its own type"))
    }

    /// The same repository, whose commands, and `status_took`, also tell `observer` what they did:
    /// for tests that check which commands a feature runs, or change the repository between them
    /// (TS's tests made a `Repo` of their own with `{ ...repo, read: ... }`).
    #[cfg(test)]
    pub(crate) fn observed(&self, observer: impl Fn(&Observed) + Send + Sync + 'static) -> Repo {
        Repo {
            inner: Arc::new(RepoInner {
                path: self.inner.path.clone(),
                shared: Arc::clone(&self.inner.shared),
                state: Arc::clone(&self.inner.state),
                observer: Some(Arc::new(observer)),
            }),
        }
    }
}

/// How a `GitCommand` runs its commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Via {
    /// Straight away: reads.
    Now,
    /// Straight away, inside `Repo::exclusive`.
    Exclusive,
    /// Each one as a write of its own, after the earlier ones.
    Writes,
    /// Each one as a fetch of its own, after the earlier ones, stopped if Gitto exits.
    Fetches,
    /// Straight away, inside `Repo::fetching`, stopped if Gitto exits.
    Fetching,
}

/// Runs git commands in a repository (TS's `GitCommand`, `(args, options) => Promise<string>`): as
/// reads (`Repo::git`), writes (`Repo::writer`), fetches (`Repo::fetcher`), or inside
/// `Repo::exclusive` or `Repo::fetching`, whichever it came from. Cheap to clone.
///
/// Each method takes the arguments when it's called and returns a `'static` future; git starts
/// when it's first polled, and is stopped, with all it started, if it's dropped before git exits.
/// A write's or fetch's place in line is taken when it's first polled too, not when it's made:
/// futures made ahead can be awaited in any order, and ones polled together (with `join!`, say)
/// run in the order they're first polled.
#[derive(Clone)]
pub struct GitCommand {
    repo: Arc<RepoInner>,
    via: Via,
}

impl GitCommand {
    /// The repository's path, which the commands run in.
    pub fn path(&self) -> &Path {
        &self.repo.path
    }

    /// Runs `args`; resolves to stdout, decoded as UTF-8 (lossily).
    pub fn run<I, S>(&self, args: I) -> BoxFuture<'static, Result<String>>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.run_with(args, RunOptions::default())
    }

    /// Runs `args` with `options`; resolves to stdout, decoded as UTF-8 (lossily).
    pub fn run_with<I, S>(&self, args: I, options: RunOptions) -> BoxFuture<'static, Result<String>>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let output = self.run_bytes_with(args, options);
        Box::pin(async move { Ok(String::from_utf8_lossy(&output.await?).into_owned()) })
    }

    /// Runs `args`; resolves to stdout as bytes (TS's `binary`), for paths handed back to git as it
    /// gave them.
    pub fn run_bytes<I, S>(&self, args: I) -> BoxFuture<'static, Result<Vec<u8>>>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.run_bytes_with(args, RunOptions::default())
    }

    /// Runs `args` with `options`; resolves to stdout as bytes (TS's `binary`).
    pub fn run_bytes_with<I, S>(
        &self,
        args: I,
        mut options: RunOptions,
    ) -> BoxFuture<'static, Result<Vec<u8>>>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let args = os_args(args);
        #[cfg(test)]
        let observed = self.repo.observer.clone().map(|observer| {
            let how = match self.via {
                Via::Now => How::Read,
                Via::Exclusive => How::Exclusive,
                Via::Writes => How::Write,
                Via::Fetches => How::Fetch,
                Via::Fetching => How::Fetching,
            };
            let args = args.iter().map(|arg| arg.to_string_lossy().into_owned());
            (
                observer,
                Observed::Ran {
                    how,
                    args: args.collect(),
                },
            )
        });
        let repo = Arc::clone(&self.repo);
        let run: BoxFuture<'static, Result<Vec<u8>>> = match self.via {
            Via::Now | Via::Exclusive => Box::pin(run_now(repo, args, options)),
            Via::Writes => {
                let turn = repo.shared.writes.turn(repo.path.clone());
                Box::pin(async move {
                    let _turn = turn.await;
                    run_now(repo, args, options).await
                })
            }
            Via::Fetches => {
                options.stop_on_exit = true;
                let turn = repo.shared.fetches.turn(repo.path.clone());
                Box::pin(async move {
                    let _turn = turn.await;
                    run_now(repo, args, options).await
                })
            }
            Via::Fetching => {
                options.stop_on_exit = true;
                Box::pin(run_now(repo, args, options))
            }
        };
        #[cfg(test)]
        if let Some((observer, observed)) = observed {
            return Box::pin(async move {
                let output = run.await;
                observer(&observed);
                output
            });
        }
        run
    }
}

/// Schedules the repository's index refresh when dropped (see `RunOptions::rewrites_files`).
struct RefreshIndex(Arc<RepoInner>);

impl Drop for RefreshIndex {
    fn drop(&mut self) {
        self.0.shared.index_refreshes.schedule(&self.0.path);
    }
}

async fn run_now(
    repo: Arc<RepoInner>,
    args: Vec<OsString>,
    options: RunOptions,
) -> Result<Vec<u8>> {
    // Also after a failure, or being cancelled: it may have rewritten some of them all the same.
    let _refresh = options
        .rewrites_files
        .then(|| RefreshIndex(Arc::clone(&repo)));
    run_git_bytes(&repo.path, args, options).await
}

/// Whether the command `args` exits with 0 rather than 1, for one that answers a yes-or-no
/// question that way (`merge-base --is-ancestor`, say); fails if it failed otherwise.
pub async fn succeeds<I, S>(run: &GitCommand, args: I) -> Result<bool>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    match run.run_bytes(args).await {
        Ok(_) => Ok(true),
        Err(error) if error.is_exit(1) => Ok(false),
        Err(error) => Err(error),
    }
}

/// The commit `rev` (e.g. `HEAD`, a branch) points at; `None` if none. Fails if git couldn't tell.
pub async fn resolve_ref(run: &GitCommand, rev: &str) -> Result<Option<String>> {
    let commit = format!("{rev}^{{commit}}");
    match run.run(["rev-parse", "--verify", "--quiet", &commit]).await {
        Ok(sha) => Ok(Some(sha.trim().to_owned())),
        // Exits with 1, saying nothing, when there's no such commit.
        Err(error) if error.is_exit(1) => Ok(None),
        Err(error) => Err(error),
    }
}

/// Whether `rev` (e.g. `HEAD`, `MERGE_HEAD`) points at a commit; fails if git couldn't tell.
pub async fn ref_exists(run: &GitCommand, rev: &str) -> Result<bool> {
    Ok(resolve_ref(run, rev).await?.is_some())
}

/// Whether the index has unmerged files, as after a merge or pop that conflicted.
pub async fn has_conflicts(run: &GitCommand) -> Result<bool> {
    Ok(!run.run_bytes(["ls-files", "--unmerged"]).await?.is_empty())
}

/// The checked-out branch's name; `None` when HEAD is detached.
pub async fn current_branch(run: &GitCommand) -> Result<Option<String>> {
    // The full name: `--short` would make it `heads/main` if there's also a tag called `main`.
    let head = match run.run(["symbolic-ref", "--quiet", "HEAD"]).await {
        Ok(head) => head,
        Err(error) if error.is_exit(1) => return Ok(None),
        Err(error) => return Err(error),
    };
    Ok(head.trim().strip_prefix("refs/heads/").map(str::to_owned))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{Duration, SystemTime};

    use super::*;
    use crate::core::errors::GitErrorKind;
    use crate::core::runner::run_git;
    use crate::test_support::conflicts::{commit_files, files};
    use crate::test_support::{Fixture, git, wait_for};

    #[test]
    fn rejects_unknown_repositories() {
        let fx = Fixture::new();
        smol::block_on(async {
            let error = fx.repos.open("missing").await.err().unwrap();
            assert!(
                matches!(error, Error::RepositoryNotFound { .. }),
                "{error:?}"
            );
        });
    }

    #[test]
    fn rejects_repositories_whose_folder_was_deleted() {
        let fx = Fixture::new();
        let path = fx.create_repo("deleted");
        fs::remove_dir_all(&path).unwrap();
        smol::block_on(async {
            let error = fx.repos.open("deleted").await.err().unwrap();
            assert!(matches!(error, Error::FolderNotFound { .. }), "{error:?}");
            assert_eq!(
                error.to_string(),
                format!("{} no longer exists.", path.display())
            );
        });
    }

    /// A repository of a thousand files, whose index was written an hour before them, as far as
    /// git can tell, or after them (`before` false): before, it can't vouch for any of them, as in
    /// a fresh clone. Returns its path, and when its index was written.
    fn create_cloned_repo(
        fx: &Fixture,
        name: &str,
        before: bool,
    ) -> (PathBuf, impl Fn() -> SystemTime) {
        let path = fx.create_repo(name);
        for i in 0..1000 {
            fs::write(path.join(format!("file-{i}.txt")), "x\n").unwrap();
        }
        git(&path, ["add", "."]);
        git(&path, ["commit", "-q", "-m", "First"]);
        let index = path.join(".git").join("index");
        let written = if before {
            SystemTime::now() - Duration::from_secs(3600)
        } else {
            SystemTime::now() + Duration::from_secs(10)
        };
        fs::File::options()
            .write(true)
            .open(&index)
            .unwrap()
            .set_modified(written)
            .unwrap();
        (path, move || {
            fs::metadata(&index).unwrap().modified().unwrap()
        })
    }

    #[test]
    fn refreshes_the_index_after_a_slow_first_status_when_it_cant_vouch_for_its_files() {
        let fx = Fixture::new();
        let (_, written) = create_cloned_repo(&fx, "refresh-slow", true);
        smol::block_on(async {
            let repo = fx.repos.open("refresh-slow").await.unwrap();
            let before = written();
            repo.status_took(Duration::from_millis(600));
            wait_for(Duration::from_secs(3), || written() > before).await;
        });
    }

    #[test]
    fn doesnt_refresh_the_index_after_a_first_status_that_was_slow_for_another_reason() {
        let fx = Fixture::new();
        let (_, written) = create_cloned_repo(&fx, "refresh-sure", false);
        smol::block_on(async {
            let repo = fx.repos.open("refresh-sure").await.unwrap();
            let before = written();
            repo.status_took(Duration::from_millis(600));
            async_io::Timer::after(Duration::from_millis(1500)).await;
            assert_eq!(written(), before);
        });
    }

    #[test]
    fn doesnt_refresh_the_index_after_a_quick_first_status_nor_a_later_slow_one() {
        let fx = Fixture::new();
        let (_, written) = create_cloned_repo(&fx, "refresh-quick", true);
        smol::block_on(async {
            let repo = fx.repos.open("refresh-quick").await.unwrap();
            let before = written();
            repo.status_took(Duration::from_millis(20));
            repo.status_took(Duration::from_millis(600));
            async_io::Timer::after(Duration::from_millis(1500)).await;
            assert_eq!(written(), before);
        });
    }

    #[test]
    fn refreshes_the_index_after_a_command_that_rewrites_files_also_one_that_fails() {
        let fx = Fixture::new();
        let (_, written) = create_cloned_repo(&fx, "refresh-failed", true);
        smol::block_on(async {
            let repo = fx.repos.open("refresh-failed").await.unwrap();
            let before = written();
            let switched = repo
                .write_with(
                    ["switch", "missing"],
                    RunOptions::default().rewrites_files(),
                )
                .await;
            assert!(switched.is_err());
            wait_for(Duration::from_secs(3), || written() > before).await;
        });
    }

    #[test]
    fn recognises_a_folder_thats_no_longer_a_repository() {
        let fx = Fixture::new();
        let path = fx.create_repo("unrepo");
        smol::block_on(async {
            let repo = fx.repos.open("unrepo").await.unwrap();
            fs::remove_dir_all(path.join(".git")).unwrap();
            let error = repo.read(["status"]).await.unwrap_err();
            let git = error.as_git().unwrap();
            assert_eq!(git.kind, GitErrorKind::NotARepository);
            assert_eq!(
                error.to_string(),
                format!("{} is no longer a git repository.", path.display())
            );
        });
    }

    /// Whether the process `pid` is still running (a zombie isn't).
    #[cfg(target_os = "linux")]
    fn is_running(pid: i32) -> bool {
        fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
            // The state follows the command's name, in parentheses.
            let state = stat.rsplit_once(") ").map(|(_, rest)| rest.chars().next());
            !matches!(state, Some(Some('Z' | 'X')))
        })
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn stops_git_and_what_it_started_when_cancelled() {
        let fx = Fixture::new();
        let path = fx.create_repo("cancelled");
        let pid_file = path.join("pid");
        smol::block_on(async {
            let repo = fx.repos.open("cancelled").await.unwrap();
            let alias = format!(
                "alias.wait=!echo $$ > '{}'; exec sleep 30",
                pid_file.display()
            );
            let mut waiting = repo.read_with(["wait"], RunOptions::default().config(alias));
            // Started by being polled; then left to run until it's written its pid.
            assert!(
                futures_lite::future::poll_once(&mut waiting)
                    .await
                    .is_none()
            );
            wait_for(Duration::from_secs(5), || {
                fs::read_to_string(&pid_file).is_ok_and(|pid| pid.ends_with('\n'))
            })
            .await;
            let pid: i32 = fs::read_to_string(&pid_file)
                .unwrap()
                .trim()
                .parse()
                .unwrap();
            assert!(is_running(pid));

            drop(waiting);
            wait_for(Duration::from_secs(5), || !is_running(pid)).await;
        });
    }

    #[test]
    fn exclusive_writes_keep_other_writes_out_until_theyre_done() {
        let fx = Fixture::new();
        fx.create_repo("exclusive");
        smol::block_on(async {
            let repo = fx.repos.open("exclusive").await.unwrap();
            let order = &Mutex::new(Vec::new());
            let (finish, finished) = async_channel::bounded::<()>(1);
            let finished = &finished;
            let exclusive = repo.exclusive(|run| async move {
                run.run(["status"]).await.unwrap();
                finished.recv().await.unwrap();
                lock(order).push("exclusive");
            });
            let write = repo.write(["status"]);
            let write = async {
                write.await.unwrap();
                lock(order).push("write");
            };
            let release = async {
                async_io::Timer::after(Duration::from_millis(200)).await;
                finish.send(()).await.unwrap();
            };
            // Polled in this order, and so run in it.
            futures::join!(exclusive, write, release);
            assert_eq!(*lock(order), ["exclusive", "write"]);
        });
    }

    #[test]
    fn writes_made_ahead_can_be_awaited_in_any_order() {
        let fx = Fixture::new();
        fx.create_repo("ahead");
        smol::block_on(async {
            let repo = fx.repos.open("ahead").await.unwrap();
            let first = repo.write(["status"]);
            let exclusive = repo.exclusive(|run| async move { run.run(["status"]).await });
            let second = repo.write(["status"]);
            // None waits on one made before it that isn't polled.
            second.await.unwrap();
            exclusive.await.unwrap();
            first.await.unwrap();
        });
    }

    #[test]
    fn settings_for_one_command_apply_to_it_and_stay_out_of_its_arguments() {
        let fx = Fixture::new();
        let path = fx.create_repo("settings");
        smol::block_on(async {
            let value = run_git(
                &path,
                ["config", "--get", "gitto.test"],
                RunOptions::default().config("gitto.test=1"),
            )
            .await
            .unwrap();
            assert_eq!(value, "1\n");
            let error = run_git(
                &path,
                ["rev-parse", "--verify", "nope"],
                RunOptions::default().config("gc.auto=0"),
            )
            .await
            .unwrap_err();
            assert_eq!(
                error.as_git().unwrap().args,
                ["rev-parse", "--verify", "nope"]
            );
        });
    }

    #[test]
    fn helpers_answer_from_git() {
        let fx = Fixture::new();
        let path = fx.create_repo("helpers");
        smol::block_on(async {
            let repo = fx.repos.open("helpers").await.unwrap();
            let run = repo.git();
            assert!(!repo.has_head().await);
            assert_eq!(resolve_ref(&run, "HEAD").await.unwrap(), None);
            assert_eq!(current_branch(&run).await.unwrap().as_deref(), Some("main"));

            commit_files(&path, &files([("a.txt", "a\n")]), "first");
            let head = git(&path, ["rev-parse", "HEAD"]);
            assert!(repo.has_head().await);
            assert_eq!(resolve_ref(&run, "main").await.unwrap(), Some(head.clone()));
            assert!(ref_exists(&run, "HEAD").await.unwrap());
            assert!(!ref_exists(&run, "MERGE_HEAD").await.unwrap());
            assert!(
                succeeds(&run, ["merge-base", "--is-ancestor", "HEAD", "main"])
                    .await
                    .unwrap()
            );
            assert!(!has_conflicts(&run).await.unwrap());

            git(&path, ["checkout", "-q", "--detach"]);
            assert_eq!(current_branch(&run).await.unwrap(), None);
        });
    }

    #[test]
    fn observed_repositories_tell_what_they_ran() {
        let fx = Fixture::new();
        fx.create_repo("observed");
        smol::block_on(async {
            let repo = fx.repos.open("observed").await.unwrap();
            let seen = Arc::new(Mutex::new(Vec::new()));
            let told = Arc::clone(&seen);
            let observed = repo.observed(move |what| lock(&told).push(what.clone()));
            observed.read(["status"]).await.unwrap();
            observed
                .exclusive(|run| async move { run.run(["status"]).await })
                .await
                .unwrap();
            observed.status_took(Duration::from_millis(20));
            observed.mark_log_needs_sorting();
            let ran = |how, args: &[&str]| Observed::Ran {
                how,
                args: args.iter().map(|arg| arg.to_string()).collect(),
            };
            assert_eq!(
                *lock(&seen),
                [
                    ran(How::Read, &["status"]),
                    ran(How::Exclusive, &["status"]),
                    Observed::StatusTook(Duration::from_millis(20)),
                ]
            );
            // The same repository, as far as what's kept for it goes.
            assert!(repo.sorts_log());
        });
    }

    #[test]
    fn keeps_one_of_each_thing_per_repository() {
        #[derive(Default)]
        struct Count(Mutex<u32>);

        let fx = Fixture::new();
        fx.create_repo("kept");
        smol::block_on(async {
            let repo = fx.repos.open("kept").await.unwrap();
            *lock(&repo.keep::<Count>().0) += 1;
            let again = fx.repos.open("kept").await.unwrap();
            assert_eq!(*lock(&again.keep::<Count>().0), 1);
        });
    }
}
