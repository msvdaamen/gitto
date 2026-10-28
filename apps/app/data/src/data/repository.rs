//! A repository as the app shows it: [`Repository`], an entity holding what's loaded of it, and
//! [`Repositories`], the global that has one for each repository opened. It takes the place of
//! TanStack Query's cache, which the Electron app kept it in by string keys: here each thing loaded
//! is a field, a [`Loaded`], or a [`Keyed`] of them for what's loaded by key, like a file's patch.
//!
//! A view reads the fields in `render`, and subscribes to [`Changed`] for the [`Part`]s it shows,
//! to render again when they change, and only then: a load that brings the same data keeps it, and
//! says nothing changed but that it's done loading. That's the one way a view hears of a change:
//! the entity never notifies, which would render again every view that read it, whatever it
//! read of it. It asks for what it shows with [`Repository::show`] (and `hold_*` for what's
//! loaded by key), which loads it unless it's loaded. Git runs on the background executor, through
//! [`RepositoryGit`]; loading something again drops the load under way, which cancels it.
//!
//! What's loaded is kept up to date by `super::watch::RepositoryWatcher`, which reloads it when the
//! repository changes on disk (see [`Repository::reload_all`]), and nothing else does: not focus,
//! nor a timer. What never changes, a commit and what's in it, is kept, a few at a time, for
//! whichever view shows it next.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use futures::FutureExt as _;
use futures::channel::oneshot;
use futures::future::{BoxFuture, join_all};
use gitto_git::features::conflicts::schema::Conflict;
use gitto_git::features::diff::schema::ChangedFile;
use gitto_git::features::history::commands::Page;
use gitto_git::features::history::schema::Commit;
use gitto_git::features::operation::schema::Operation;
use gitto_git::features::overview::schema::Overview;
use gitto_git::features::refs::schema::Ref;
use gitto_git::features::remote::schema::PushTarget;
use gitto_git::features::stash::schema::Stash;
use gitto_git::features::status::schema::{StatusReply, Uncommitted};
use gitto_git::features::worktree::schema::Worktree;
use gitto_git::input::{FileInput, FullSha, NonEmpty, RepositoryId, Sha};
use gitto_git::schema::FileStatus;
use gpui_kit::{App, AppContext as _, Context, Entity, EventEmitter, Global, Subscription, Task};

use super::changes::{Change, ChangeGroup};
use super::diff::{ChangedFiles, FilesOf};
use super::error::DataError;
use super::file_diff::FilePatch;
use super::git::{GitRepository, GitTask, RepositoryGit};
use super::history::{Log, NEXT_PAGE, followed_by, reload_limit};
use super::keyed::{Hold, Keyed};
use super::loaded::{LoadState, Loaded};
use super::opaque::Opaque;
use crate::services::Services;
use gitto_ui_logic::git::diff_source::{DiffSource, diff_file_key, is_uncommitted};

/// How many commits, and of each thing in them, are kept that nothing shows: enough to step back
/// and forth through the history without loading them again.
const KEPT_COMMITS: usize = 64;

/// How many patches and conflicted files are kept that nothing shows: they can be megabytes, and
/// one is loaded ahead whenever the pointer rests on a file.
const KEPT_FILES: usize = 16;

/// What of a repository changed (see [`Changed`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Part {
    /// The status, with the uncommitted changes.
    Status,
    /// The merge, rebase, cherry-pick or revert under way.
    Operation,
    Log,
    Refs,
    Remotes,
    Stashes,
    Worktrees,
    /// Where the Push button pushes, and how many commits.
    PushTarget,
    /// What the home page shows of it, beyond its status.
    Overview,
    /// Which remote branch has a commit.
    PushedTo,
    /// A file's patch, in a commit, a stash or the uncommitted changes.
    Patches,
    Conflicts,
    /// A commit's details, files or message.
    Commits,
    /// Loading more of the log.
    More,
    /// Whether a [`Change`] to the repository (a pull, say) is running, and why it last failed.
    Changes,
}

impl Part {
    /// The field it's of, if it's one's.
    pub fn field(self) -> Option<Field> {
        Some(match self {
            Part::Status => Field::Status,
            Part::Operation => Field::Operation,
            Part::Log => Field::Log,
            Part::Refs => Field::Refs,
            Part::Remotes => Field::Remotes,
            Part::Stashes => Field::Stashes,
            Part::Worktrees => Field::Worktrees,
            Part::PushTarget => Field::PushTarget,
            Part::Overview => Field::Overview,
            Part::PushedTo
            | Part::Patches
            | Part::Conflicts
            | Part::Commits
            | Part::More
            | Part::Changes => return None,
        })
    }
}

/// What a repository holds one of, and loads whole: what a view asks for with
/// [`Repository::show`], and loads again with [`Repository::reload`]. What's loaded by key, like a
/// file's patch, a view holds instead (`hold_*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Field {
    Status,
    Operation,
    Log,
    Refs,
    Remotes,
    Stashes,
    Worktrees,
    PushTarget,
    Overview,
}

impl Field {
    pub const ALL: [Field; 9] = [
        Field::Status,
        Field::Operation,
        Field::Log,
        Field::Refs,
        Field::Remotes,
        Field::Stashes,
        Field::Worktrees,
        Field::PushTarget,
        Field::Overview,
    ];

    /// What a repository says changed of it.
    pub fn part(self) -> Part {
        match self {
            Field::Status => Part::Status,
            Field::Operation => Part::Operation,
            Field::Log => Part::Log,
            Field::Refs => Part::Refs,
            Field::Remotes => Part::Remotes,
            Field::Stashes => Part::Stashes,
            Field::Worktrees => Part::Worktrees,
            Field::PushTarget => Part::PushTarget,
            Field::Overview => Part::Overview,
        }
    }
}

/// What a [`Repository`] says when what it holds of `Part` changed: its data, why it failed, or
/// whether it's loading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Changed(pub Part);

/// Whether a change is running, and why it last failed: one of a repository's (see
/// [`Repository::run`]), or a view's own (see `super::mutation`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChangeState {
    pub running: bool,
    pub error: Option<DataError>,
}

/// The loads a reload started: `.await` it to wait for them, or drop it not to (they run either
/// way).
#[must_use = "awaiting it waits for the reloads; drop it not to"]
pub struct Reloading(Vec<oneshot::Receiver<()>>);

impl Reloading {}

impl std::future::IntoFuture for Reloading {
    type Output = ();
    type IntoFuture = BoxFuture<'static, ()>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            join_all(self.0).await;
        })
    }
}

/// Where a file's patch is kept: its source and path.
type PatchKey = (DiffSource, String);

/// A repository's data (see the module's doc).
pub struct Repository {
    id: RepositoryId,
    git: Rc<dyn RepositoryGit>,
    status: Loaded<Opaque<Uncommitted>>,
    operation: Loaded<Option<Operation>>,
    log: Loaded<Log>,
    refs: Loaded<Arc<[Ref]>>,
    remotes: Loaded<Vec<String>>,
    stashes: Loaded<Arc<[Stash]>>,
    worktrees: Loaded<Arc<[Worktree]>>,
    push_target: Loaded<PushTarget>,
    overview: Loaded<Overview>,
    pushed_to: Keyed<String, Option<String>>,
    patches: Keyed<PatchKey, FilePatch>,
    conflicts: Keyed<String, Conflict>,
    /// Files' contents at a version, by their object id, which never change. Only a few are kept:
    /// one can be megabytes, it's only read to fill in a patch, which keeps what it needs of it,
    /// and git reads it again in milliseconds.
    blobs: Keyed<String, String>,
    commits: Keyed<String, Commit>,
    files: Keyed<(FilesOf, String), ChangedFiles>,
    messages: Keyed<String, String>,
    more: LoadingMore,
    changes: HashMap<Change, ChangeState>,
}

/// Loading more of the log: whether it's under way, and why it last failed, both of the log as it
/// was then, so forgotten once it reloads.
#[derive(Default)]
struct LoadingMore {
    task: Option<Task<()>>,
    error: Option<DataError>,
}

impl EventEmitter<Changed> for Repository {}

impl Repository {
    /// The repository `id`, asking `git` for what it loads. Nothing's loaded until it's shown.
    pub fn new(id: RepositoryId, git: Rc<dyn RepositoryGit>) -> Self {
        Repository {
            id,
            git,
            status: Loaded::default(),
            operation: Loaded::default(),
            log: Loaded::default(),
            refs: Loaded::default(),
            remotes: Loaded::default(),
            stashes: Loaded::default(),
            worktrees: Loaded::default(),
            push_target: Loaded::default(),
            overview: Loaded::default(),
            pushed_to: Keyed::new(KEPT_COMMITS),
            patches: Keyed::new(KEPT_FILES),
            conflicts: Keyed::new(KEPT_FILES),
            blobs: Keyed::new(KEPT_FILES),
            commits: Keyed::new(KEPT_COMMITS),
            files: Keyed::new(KEPT_COMMITS),
            messages: Keyed::new(KEPT_COMMITS),
            more: LoadingMore::default(),
            changes: HashMap::new(),
        }
    }

    pub fn id(&self) -> &RepositoryId {
        &self.id
    }

    /// What it asks git.
    pub fn git(&self) -> &Rc<dyn RepositoryGit> {
        &self.git
    }

    /// Asks `git` from now on: for tests, which make up what a repository's git answers.
    pub fn set_git(&mut self, git: Rc<dyn RepositoryGit>) {
        self.git = git;
    }

    pub fn status(&self) -> &Loaded<Opaque<Uncommitted>> {
        &self.status
    }

    pub fn operation(&self) -> &Loaded<Option<Operation>> {
        &self.operation
    }

    pub fn log(&self) -> &Loaded<Log> {
        &self.log
    }

    pub fn refs(&self) -> &Loaded<Arc<[Ref]>> {
        &self.refs
    }

    pub fn remotes(&self) -> &Loaded<Vec<String>> {
        &self.remotes
    }

    pub fn stashes(&self) -> &Loaded<Arc<[Stash]>> {
        &self.stashes
    }

    pub fn worktrees(&self) -> &Loaded<Arc<[Worktree]>> {
        &self.worktrees
    }

    pub fn push_target(&self) -> &Loaded<PushTarget> {
        &self.push_target
    }

    pub fn overview(&self) -> &Loaded<Overview> {
        &self.overview
    }

    /// Loads `field` unless it's loaded, or loading: for a view about to show it.
    pub fn show(&mut self, field: Field, cx: &mut Context<Self>) {
        let state = self.state(field);
        if state.is_stale() && !state.is_fetching() {
            drop(self.reload(field, cx));
        }
    }

    /// Where `field` is in loading, whatever it holds.
    fn state(&self, field: Field) -> &dyn LoadState {
        match field {
            Field::Status => &self.status,
            Field::Operation => &self.operation,
            Field::Log => &self.log,
            Field::Refs => &self.refs,
            Field::Remotes => &self.remotes,
            Field::Stashes => &self.stashes,
            Field::Worktrees => &self.worktrees,
            Field::PushTarget => &self.push_target,
            Field::Overview => &self.overview,
        }
    }

    /// Loads `field` again now, in place of (and so cancelling) a load under way; resolves once
    /// it's done.
    pub fn reload(&mut self, field: Field, cx: &mut Context<Self>) -> Reloading {
        let git = Rc::clone(&self.git);
        let part = field.part();
        let wait = match field {
            Field::Status => {
                let previous = self.status.data().cloned();
                let since = previous.as_ref().map(|status| status.version.clone());
                let reply = git.status(since);
                self.start(
                    part,
                    |repo| Some(&mut repo.status),
                    async move {
                        match reply.await? {
                            // Only ever the answer when there was a previous status to compare
                            // with.
                            StatusReply::Unchanged { .. } => previous.ok_or_else(|| {
                                DataError::msg("The status was unchanged, from none.")
                            }),
                            StatusReply::Status(status) => Ok(Opaque::new(*status)),
                        }
                    }
                    .boxed(),
                    cx,
                )
            }
            Field::Operation => {
                self.start(part, |repo| Some(&mut repo.operation), git.operation(), cx)
            }
            Field::Log => {
                let loaded = self.log.data().map_or(0, |log| log.commits.len());
                let fetch = log_page(&*git, &self.id, 0, reload_limit(loaded));
                // Loading more of the log as it was would leave it out of order.
                if self.more.task.take().is_some() || self.more.error.take().is_some() {
                    cx.emit(Changed(Part::More));
                }
                self.start(part, |repo| Some(&mut repo.log), fetch, cx)
            }
            Field::Refs => {
                let refs = git.refs();
                let fetch = async move { Ok(refs.await?.into()) }.boxed();
                self.start(part, |repo| Some(&mut repo.refs), fetch, cx)
            }
            Field::Remotes => self.start(part, |repo| Some(&mut repo.remotes), git.remotes(), cx),
            Field::Stashes => {
                let stashes = git.stashes();
                let fetch = async move { Ok(stashes.await?.into()) }.boxed();
                self.start(part, |repo| Some(&mut repo.stashes), fetch, cx)
            }
            Field::Worktrees => {
                let worktrees = git.worktrees();
                let fetch = async move { Ok(worktrees.await?.into()) }.boxed();
                self.start(part, |repo| Some(&mut repo.worktrees), fetch, cx)
            }
            Field::PushTarget => self.start(
                part,
                |repo| Some(&mut repo.push_target),
                git.push_target(),
                cx,
            ),
            Field::Overview => {
                self.start(part, |repo| Some(&mut repo.overview), git.overview(), cx)
            }
        };
        Reloading(wait.into_iter().collect())
    }

    /// Loads, with `fetch`, what `at` finds, in place of a load under way; a receiver told once
    /// it's done. Says it `Changed` as it starts loading, unless it was loading already, and as
    /// it's done.
    fn start<T: PartialEq + Send + 'static>(
        &mut self,
        part: Part,
        at: impl Fn(&mut Self) -> Option<&mut Loaded<T>> + 'static,
        fetch: GitTask<T>,
        cx: &mut Context<Self>,
    ) -> Option<oneshot::Receiver<()>> {
        let at = Rc::new(at);
        let fetch = cx.background_spawn(fetch);
        let settle = Rc::clone(&at);
        let task = cx.spawn(async move |this, cx| {
            let result = fetch.await;
            let _ = this.update(cx, |repo, cx| {
                if let Some(loaded) = settle(repo) {
                    loaded.settle(result);
                    cx.emit(Changed(part));
                }
            });
        });
        let loaded = at(self)?;
        let was_loading = loaded.is_fetching();
        loaded.start(task);
        let wait = loaded.wait();
        if !was_loading {
            cx.emit(Changed(part));
        }
        wait
    }

    /// Loads again what's shown of the repository, after it changed on disk, or a change to it (a
    /// pull, say); what nothing shows is loaded again once something does. What the home page
    /// shows of it (the overview) is only loaded there, when it's shown.
    pub fn reload_all(&mut self, cx: &mut Context<Self>) -> Reloading {
        let mut waits = self.reload_uncommitted(cx).0;
        for field in Field::ALL {
            match field {
                // Reloaded with the uncommitted changes.
                Field::Status => {}
                // Loaded where it's shown (see above).
                Field::Overview => self.overview.mark_stale(),
                _ if self.state(field).is_used() => waits.extend(self.reload(field, cx).0),
                _ => {}
            }
        }
        for sha in self.pushed_to.held_and_drop_rest(|_| true) {
            waits.extend(self.load_pushed_to(sha, cx));
        }
        Reloading(waits)
    }

    /// Loads again what's shown of the uncommitted changes, after a file changed, or the index: the
    /// status, and the patches and conflicted files on show.
    pub fn reload_uncommitted(&mut self, cx: &mut Context<Self>) -> Reloading {
        self.reload_uncommitted_but(None, cx)
    }

    /// [`reload_uncommitted`](Self::reload_uncommitted), but for the patch `kept`, which is up to
    /// date: one a change just told.
    pub(crate) fn reload_uncommitted_but(
        &mut self,
        kept: Option<PatchKey>,
        cx: &mut Context<Self>,
    ) -> Reloading {
        let mut waits = Vec::new();
        if self.status.is_used() {
            waits.extend(self.reload(Field::Status, cx).0);
        }
        let patches = self.patches.held_and_drop_rest(|key| {
            is_uncommitted(&key.0) && kept.as_ref().is_none_or(|kept| kept != key)
        });
        for key in patches {
            let file = self
                .patches
                .get(&key)
                .and_then(Loaded::data)
                .map(|patch| patch.file.clone());
            if let Some(file) = file {
                waits.extend(self.load_patch(key.0, file, cx));
            }
        }
        for path in self.conflicts.held_and_drop_rest(|_| true) {
            waits.extend(self.load_conflict(path, cx));
        }
        cx.emit(Changed(Part::Patches));
        Reloading(waits)
    }

    /// Loads again what was loaded before the repository came on show, when nothing watched it,
    /// and may be out of date; not what's loading, which is loaded since.
    pub fn reload_when_shown(&mut self, cx: &mut Context<Self>) {
        for field in Field::ALL {
            if field == Field::Overview {
                self.overview.mark_stale();
                continue;
            }
            let state = self.state(field);
            if state.is_used() && !state.is_fetching() {
                drop(self.reload(field, cx));
            }
        }
        drop(self.pushed_to.held_and_drop_rest(|_| true));
        drop(
            self.patches
                .held_and_drop_rest(|(source, _)| is_uncommitted(source)),
        );
        drop(self.conflicts.held_and_drop_rest(|_| true));
    }

    // Loading more of the log.

    /// Whether more of the log is loading.
    pub fn is_loading_more(&self) -> bool {
        self.more.task.is_some()
    }

    /// Why loading more of the log last failed, if it did, since it last reloaded.
    pub fn more_error(&self) -> Option<&DataError> {
        self.more.error.as_ref()
    }

    /// Whether there's more of the log to load, once it's loaded.
    pub fn has_more(&self) -> bool {
        self.log.data().is_some_and(|log| !log.complete)
    }

    /// Loads `NEXT_PAGE` more commits of the log, after those loaded, unless it's all loaded, or
    /// loading, or reloading, which would leave that unused.
    pub fn load_more(&mut self, cx: &mut Context<Self>) {
        if !self.has_more() || self.log.is_fetching() || self.is_loading_more() {
            return;
        }
        let Some(log) = self.log.data().cloned() else {
            return;
        };
        let page =
            cx.background_spawn(log_commits(&*self.git, log.commits.len() as u32, NEXT_PAGE));
        self.more.error = None;
        self.more.task = Some(cx.spawn(async move |this, cx| {
            let result = page.await;
            let _ = this.update(cx, |repo, cx| {
                // A reload since dropped this task, so this follows on from what's there.
                repo.more.task = None;
                match result {
                    Ok(commits) => {
                        let complete = commits.len() < NEXT_PAGE as usize;
                        repo.log.set(followed_by(&log, commits, complete));
                        cx.emit(Changed(Part::Log));
                    }
                    Err(error) => repo.more.error = Some(error),
                }
                cx.emit(Changed(Part::More));
            });
        }));
        cx.emit(Changed(Part::More));
    }

    // What's loaded by key.

    /// Which remote branch has the commit `sha`, if that's loaded.
    pub fn pushed_to(&self, sha: &str) -> Option<&Loaded<Option<String>>> {
        self.pushed_to.get(&sha.to_owned())
    }

    /// Holds which remote branch has the commit `sha`, loading it unless it's loaded or loading,
    /// and loading it again as the refs change, while it's held.
    pub fn hold_pushed_to(&mut self, sha: &str, cx: &mut Context<Self>) -> Hold {
        let hold = self.pushed_to.hold(sha.to_owned());
        let loaded = self.pushed_to.entry(sha.to_owned());
        if loaded.is_stale() && !loaded.is_fetching() {
            drop(self.load_pushed_to(sha.to_owned(), cx));
        }
        hold
    }

    fn load_pushed_to(
        &mut self,
        sha: String,
        cx: &mut Context<Self>,
    ) -> Option<oneshot::Receiver<()>> {
        let fetch = match Sha::parse(sha.clone()) {
            Ok(commit) => self.git.pushed_to(commit),
            Err(error) => failed(self.git.invalid(error)),
        };
        self.pushed_to.entry(sha.clone());
        self.start(
            Part::PushedTo,
            move |repo| repo.pushed_to.get_mut(&sha),
            fetch,
            cx,
        )
    }

    /// The patch of the file at `path` in `source`, if that's loaded.
    pub fn patch(&self, source: &DiffSource, path: &str) -> Option<&Loaded<FilePatch>> {
        self.patches.get(&(source.clone(), path.to_owned()))
    }

    /// Holds the patch of `file` in `source`, loading it unless it's loaded or loading, and
    /// loading it again as the uncommitted changes change, if it's of those, while it's held.
    pub fn hold_patch(
        &mut self,
        source: &DiffSource,
        file: &ChangedFile,
        cx: &mut Context<Self>,
    ) -> Hold {
        let key = (source.clone(), file.path.clone());
        let hold = self.patches.hold(key.clone());
        let loaded = self.patches.entry(key);
        if loaded.is_stale() && !loaded.is_fetching() {
            drop(self.load_patch(source.clone(), file.clone(), cx));
        }
        hold
    }

    /// The patch of `file` in `source`: what's loaded, or what loads; e.g. for a file about to be
    /// opened.
    pub fn fetch_patch(
        &mut self,
        source: &DiffSource,
        file: &ChangedFile,
        cx: &mut Context<Self>,
    ) -> Task<Result<FilePatch, DataError>> {
        let key = (source.clone(), file.path.clone());
        let wait = {
            let loaded = self.patches.entry(key.clone());
            if loaded.is_fetching() {
                loaded.wait()
            } else if loaded.is_stale() {
                self.load_patch(source.clone(), file.clone(), cx)
            } else {
                None
            }
        };
        cx.spawn(async move |this, cx| {
            if let Some(wait) = wait {
                let _ = wait.await;
            }
            this.read_with(cx, |repo, _| loaded_result(repo.patches.get(&key)))?
        })
    }

    fn load_patch(
        &mut self,
        source: DiffSource,
        file: ChangedFile,
        cx: &mut Context<Self>,
    ) -> Option<oneshot::Receiver<()>> {
        let input = FileInput {
            path: file.path.clone(),
            orig_path: file.orig_path.clone(),
        };
        let patch = match &source {
            DiffSource::Commit { sha } => match Sha::parse(sha.clone()) {
                Ok(commit) => self.git.commit_file_patch(commit, input),
                Err(error) => failed(self.git.invalid(error)),
            },
            DiffSource::Stash { sha } => match FullSha::parse(sha.clone()) {
                Ok(stash) => self.git.stash_file_patch(stash, input),
                Err(error) => failed(self.git.invalid(error)),
            },
            DiffSource::Unstaged => {
                let untracked = file.status == FileStatus::Untracked;
                self.git.unstaged_file_patch(input, untracked)
            }
            DiffSource::Staged => self.git.staged_file_patch(input),
        };
        let key = (source.clone(), file.path.clone());
        let fetch = async move {
            let patch = patch.await?;
            Ok(FilePatch {
                key: diff_file_key(&source, &file.path),
                file,
                patch: patch.into(),
            })
        }
        .boxed();
        self.patches.entry(key.clone());
        self.start(
            Part::Patches,
            move |repo| repo.patches.get_mut(&key),
            fetch,
            cx,
        )
    }

    /// The conflicted file at `path`, if it's loaded.
    pub fn conflict(&self, path: &str) -> Option<&Loaded<Conflict>> {
        self.conflicts.get(&path.to_owned())
    }

    /// Holds the conflicted file at `path`, loading it unless it's loaded or loading, and loading
    /// it again as the uncommitted changes change, while it's held.
    pub fn hold_conflict(&mut self, path: &str, cx: &mut Context<Self>) -> Hold {
        let hold = self.conflicts.hold(path.to_owned());
        let loaded = self.conflicts.entry(path.to_owned());
        if loaded.is_stale() && !loaded.is_fetching() {
            drop(self.load_conflict(path.to_owned(), cx));
        }
        hold
    }

    /// Loads the conflicted file at `path` again: it changed on disk since it was read, say.
    pub fn reload_conflict(
        &mut self,
        path: &str,
        cx: &mut Context<Self>,
    ) -> Option<oneshot::Receiver<()>> {
        self.load_conflict(path.to_owned(), cx)
    }

    /// The conflicted file at `path`: what's loaded, or what loads; e.g. for one about to be
    /// opened.
    pub fn fetch_conflict(
        &mut self,
        path: &str,
        cx: &mut Context<Self>,
    ) -> Task<Result<Conflict, DataError>> {
        let path = path.to_owned();
        let wait = {
            let loaded = self.conflicts.entry(path.clone());
            if loaded.is_fetching() {
                loaded.wait()
            } else if loaded.is_stale() {
                self.load_conflict(path.clone(), cx)
            } else {
                None
            }
        };
        cx.spawn(async move |this, cx| {
            if let Some(wait) = wait {
                let _ = wait.await;
            }
            this.read_with(cx, |repo, _| loaded_result(repo.conflicts.get(&path)))?
        })
    }

    fn load_conflict(
        &mut self,
        path: String,
        cx: &mut Context<Self>,
    ) -> Option<oneshot::Receiver<()>> {
        let fetch = match NonEmpty::parse(path.clone()) {
            Ok(file) => self.git.conflict(file),
            Err(error) => failed(self.git.invalid(error)),
        };
        self.conflicts.entry(path.clone());
        self.start(
            Part::Conflicts,
            move |repo| repo.conflicts.get_mut(&path),
            fetch,
            cx,
        )
    }

    /// The contents of the object `oid`, a file's at some version: what's loaded, or what loads;
    /// to show more of a file around its changes, say.
    pub fn fetch_blob(
        &mut self,
        oid: &str,
        cx: &mut Context<Self>,
    ) -> Task<Result<String, DataError>> {
        let oid = oid.to_owned();
        let wait = {
            let loaded = self.blobs.entry(oid.clone());
            if loaded.is_fetching() {
                loaded.wait()
            } else if loaded.is_stale() {
                let fetch = match FullSha::parse(oid.clone()) {
                    Ok(blob) => self.git.blob(blob),
                    Err(error) => failed(self.git.invalid(error)),
                };
                let key = oid.clone();
                self.start(
                    Part::Patches,
                    move |repo| repo.blobs.get_mut(&key),
                    fetch,
                    cx,
                )
            } else {
                None
            }
        };
        cx.spawn(async move |this, cx| {
            if let Some(wait) = wait {
                let _ = wait.await;
            }
            this.read_with(cx, |repo, _| loaded_result(repo.blobs.get(&oid)))?
        })
    }

    /// The commit `sha`, if it's loaded, or in the log: the log has all of a commit, so git isn't
    /// asked for it again.
    pub fn commit(&self, sha: &str) -> Option<&Commit> {
        self.commits
            .get(&sha.to_owned())
            .and_then(Loaded::data)
            .or_else(|| self.commit_in_log(sha))
    }

    /// Why loading the commit `sha` failed, if it did.
    pub fn commit_error(&self, sha: &str) -> Option<&DataError> {
        self.commits.get(&sha.to_owned()).and_then(Loaded::error)
    }

    fn commit_in_log(&self, sha: &str) -> Option<&Commit> {
        self.log
            .data()?
            .commits
            .iter()
            .find(|commit| commit.sha == sha)
    }

    /// Holds the commit `sha`, loading it unless it's loaded or in the log.
    pub fn hold_commit(&mut self, sha: &str, cx: &mut Context<Self>) -> Hold {
        let hold = self.commits.hold(sha.to_owned());
        if let Some(commit) = self.commit_in_log(sha).cloned() {
            self.commits.set(sha.to_owned(), commit);
        } else if self.commits.entry(sha.to_owned()).is_stale()
            && !self.commits.entry(sha.to_owned()).is_fetching()
        {
            let sha = sha.to_owned();
            let fetch = match Sha::parse(sha.clone()) {
                Ok(commit) => self.git.commit(commit),
                Err(error) => failed(self.git.invalid(error)),
            };
            drop(self.start(
                Part::Commits,
                move |repo| repo.commits.get_mut(&sha),
                fetch,
                cx,
            ));
        }
        hold
    }

    /// The files the commit or stash `sha` changed, if they're loaded.
    pub fn files(&self, of: FilesOf, sha: &str) -> Option<&Loaded<ChangedFiles>> {
        self.files.get(&(of, sha.to_owned()))
    }

    /// Holds the files the commit or stash `sha` changed, loading them unless they're loaded.
    pub fn hold_files(&mut self, of: FilesOf, sha: &str, cx: &mut Context<Self>) -> Hold {
        let key = (of, sha.to_owned());
        let hold = self.files.hold(key.clone());
        let loaded = self.files.entry(key.clone());
        if loaded.is_stale() && !loaded.is_fetching() {
            let sha = sha.to_owned();
            let files = match of {
                FilesOf::Commit => match Sha::parse(sha.clone()) {
                    Ok(commit) => self.git.commit_files(commit),
                    Err(error) => failed(self.git.invalid(error)),
                },
                FilesOf::Stash => match FullSha::parse(sha.clone()) {
                    Ok(stash) => self.git.stash_files(stash),
                    Err(error) => failed(self.git.invalid(error)),
                },
            };
            let fetch = async move {
                Ok(ChangedFiles {
                    sha,
                    files: Opaque::new(files.await?),
                })
            }
            .boxed();
            drop(self.start(
                Part::Commits,
                move |repo| repo.files.get_mut(&key),
                fetch,
                cx,
            ));
        }
        hold
    }

    /// The commit `sha`'s whole message, if it's loaded.
    pub fn message(&self, sha: &str) -> Option<&Loaded<String>> {
        self.messages.get(&sha.to_owned())
    }

    /// Holds the commit `sha`'s whole message, loading it unless it's loaded.
    pub fn hold_message(&mut self, sha: &str, cx: &mut Context<Self>) -> Hold {
        let hold = self.messages.hold(sha.to_owned());
        let loaded = self.messages.entry(sha.to_owned());
        if loaded.is_stale() && !loaded.is_fetching() {
            let sha = sha.to_owned();
            let fetch = match Sha::parse(sha.clone()) {
                Ok(commit) => self.git.commit_message(commit),
                Err(error) => failed(self.git.invalid(error)),
            };
            drop(self.start(
                Part::Commits,
                move |repo| repo.messages.get_mut(&sha),
                fetch,
                cx,
            ));
        }
        hold
    }

    // Setting what's loaded: what a change knows it changed, and tests' data.

    pub fn set_status(&mut self, status: Uncommitted, cx: &mut Context<Self>) {
        self.status.cancel();
        self.status.set(Opaque::new(status));
        self.changed(Part::Status, cx);
    }

    pub fn set_operation(&mut self, operation: Option<Operation>, cx: &mut Context<Self>) {
        self.operation.cancel();
        self.operation.set(operation);
        self.changed(Part::Operation, cx);
    }

    pub fn set_log(&mut self, log: Log, cx: &mut Context<Self>) {
        self.log.cancel();
        self.log.set(log);
        self.changed(Part::Log, cx);
    }

    pub fn set_refs(&mut self, refs: Vec<Ref>, cx: &mut Context<Self>) {
        self.refs.cancel();
        self.refs.set(refs.into());
        self.changed(Part::Refs, cx);
    }

    pub fn set_remotes(&mut self, remotes: Vec<String>, cx: &mut Context<Self>) {
        self.remotes.cancel();
        self.remotes.set(remotes);
        self.changed(Part::Remotes, cx);
    }

    pub fn set_stashes(&mut self, stashes: Vec<Stash>, cx: &mut Context<Self>) {
        self.stashes.cancel();
        self.stashes.set(stashes.into());
        self.changed(Part::Stashes, cx);
    }

    pub fn set_worktrees(&mut self, worktrees: Vec<Worktree>, cx: &mut Context<Self>) {
        self.worktrees.cancel();
        self.worktrees.set(worktrees.into());
        self.changed(Part::Worktrees, cx);
    }

    pub fn set_push_target(&mut self, target: PushTarget, cx: &mut Context<Self>) {
        self.push_target.cancel();
        self.push_target.set(target);
        self.changed(Part::PushTarget, cx);
    }

    pub fn set_pushed_to(&mut self, sha: &str, remote: Option<String>, cx: &mut Context<Self>) {
        self.pushed_to.set(sha.to_owned(), remote);
        self.changed(Part::PushedTo, cx);
    }

    /// Puts `patch` in place of the patch of the file at `path` in `source`, and of a load of it
    /// under way, keeping what it says of the file; whether there was one to replace.
    pub(crate) fn replace_patch(&mut self, source: &DiffSource, path: &str, patch: &str) -> bool {
        let Some(loaded) = self.patches.get_mut(&(source.clone(), path.to_owned())) else {
            return false;
        };
        let Some(last) = loaded.data().cloned() else {
            return false;
        };
        loaded.cancel();
        loaded.set(FilePatch {
            patch: Arc::from(patch),
            ..last
        });
        true
    }

    pub fn set_patch(&mut self, source: &DiffSource, patch: FilePatch, cx: &mut Context<Self>) {
        self.patches
            .set((source.clone(), patch.file.path.clone()), patch);
        self.changed(Part::Patches, cx);
    }

    pub fn set_conflict(&mut self, path: &str, conflict: Conflict, cx: &mut Context<Self>) {
        self.conflicts.set(path.to_owned(), conflict);
        self.changed(Part::Conflicts, cx);
    }

    pub fn set_files(&mut self, of: FilesOf, files: ChangedFiles, cx: &mut Context<Self>) {
        self.files.set((of, files.sha.clone()), files);
        self.changed(Part::Commits, cx);
    }

    pub fn set_message(&mut self, sha: &str, message: String, cx: &mut Context<Self>) {
        self.messages.set(sha.to_owned(), message);
        self.changed(Part::Commits, cx);
    }

    /// Says `part` changed (see the module's doc).
    fn changed(&mut self, part: Part, cx: &mut Context<Self>) {
        cx.emit(Changed(part));
    }

    // Changes: their state.

    /// The change's state: idle, without an error, if it's not been run, or went through.
    pub fn change_state(&self, change: Change) -> ChangeState {
        self.changes.get(&change).cloned().unwrap_or_default()
    }

    /// Whether any of `changes` is running.
    pub fn is_running(&self, changes: &[Change]) -> bool {
        changes
            .iter()
            .any(|change| self.change_state(*change).running)
    }

    /// Whether one of the changes of `group` is running.
    pub fn is_changing(&self, group: ChangeGroup) -> bool {
        self.changes
            .iter()
            .any(|(change, state)| state.running && change.is_in(group))
    }

    /// Forgets why `change` last failed, unless it's running.
    pub fn dismiss(&mut self, change: Change, cx: &mut Context<Self>) {
        if self.change_state(change).running {
            return;
        }
        if self.changes.remove(&change).is_some() {
            self.changed(Part::Changes, cx);
        }
    }

    /// Runs `change`, which `work` does, unless it's running: `None` then. Its state is kept (see
    /// [`change_state`](Self::change_state)) until it's run again, or dismissed; the receiver is
    /// told what it resolved to, for the view that ran it to act on if it's still there.
    pub fn run<R: 'static>(
        &mut self,
        change: Change,
        work: impl FnOnce(&mut Self, &mut Context<Self>) -> Task<Result<R, DataError>>,
        cx: &mut Context<Self>,
    ) -> Option<oneshot::Receiver<Result<R, DataError>>> {
        if self.change_state(change).running {
            return None;
        }
        self.changes.insert(
            change,
            ChangeState {
                running: true,
                error: None,
            },
        );
        self.changed(Part::Changes, cx);
        let work = work(self, cx);
        let (done, result) = oneshot::channel();
        cx.spawn(async move |this, cx| {
            let outcome = work.await;
            let _ = this.update(cx, |repository, cx| {
                match &outcome {
                    // Nothing to keep for one that went through.
                    Ok(_) => repository.changes.remove(&change),
                    Err(error) => repository.changes.insert(
                        change,
                        ChangeState {
                            running: false,
                            error: Some(error.clone()),
                        },
                    ),
                };
                repository.changed(Part::Changes, cx);
            });
            let _ = done.send(outcome);
        })
        .detach();
        Some(result)
    }

    /// Runs `action`, git's part of a change, on the background executor, then reloads what's
    /// shown of the repository, also after a failure: the action may have changed it partway, e.g.
    /// a pop that conflicts. Resolves once that's reloaded.
    pub fn reloading<R: Send + 'static>(
        &mut self,
        action: GitTask<R>,
        cx: &mut Context<Self>,
    ) -> Task<Result<R, DataError>> {
        let action = cx.background_spawn(action);
        cx.spawn(async move |this, cx| {
            let result = action.await;
            let reloading = this.update(cx, |repo, cx| repo.reload_all(cx));
            if let Ok(reloading) = reloading {
                reloading.await;
            }
            result
        })
    }
}

/// Renders the view `cx` is of again when what `select` picks of `repository` changes, as it
/// says `part` changed, and only then: e.g. the status's summary, which stays the same as files
/// are edited.
pub fn observe_selected<V: 'static, S: PartialEq + 'static>(
    repository: &Entity<Repository>,
    part: Part,
    select: impl Fn(&Repository) -> S + 'static,
    cx: &mut Context<V>,
) -> Subscription {
    let last = RefCell::new(select(repository.read(cx)));
    cx.subscribe(repository, move |_, repository, Changed(changed), cx| {
        if *changed != part {
            return;
        }
        let next = select(repository.read(cx));
        if *last.borrow() != next {
            *last.borrow_mut() = next;
            cx.notify();
        }
    })
}

/// Renders the view `cx` is of again when `repository` says one of `parts` changed.
pub fn observe_parts<V: 'static>(
    repository: &Entity<Repository>,
    parts: &'static [Part],
    cx: &mut Context<V>,
) -> Subscription {
    cx.subscribe(repository, move |_, _, Changed(changed), cx| {
        if parts.contains(changed) {
            cx.notify();
        }
    })
}

/// What a view shows of a repository: its entity, opened (see [`Repositories::open`]), with the
/// parts the view shows loaded, and rendering the view again as they change. A view keeps one, and
/// shows another repository's with [`set`](Self::set).
pub struct Shown {
    repository: Entity<Repository>,
    parts: &'static [Part],
    _changes: Subscription,
}

impl Shown {
    /// The repository `id`'s `parts`, for the view `cx` is of.
    pub fn new<V: 'static>(id: &RepositoryId, parts: &'static [Part], cx: &mut Context<V>) -> Self {
        let repository = Repositories::open(id, cx);
        Shown::of(repository, parts, cx)
    }

    fn of<V: 'static>(
        repository: Entity<Repository>,
        parts: &'static [Part],
        cx: &mut Context<V>,
    ) -> Self {
        repository.update(cx, |repository, cx| {
            for field in parts.iter().filter_map(|part| part.field()) {
                repository.show(field, cx);
            }
        });
        let changes = observe_parts(&repository, parts, cx);
        Shown {
            repository,
            parts,
            _changes: changes,
        }
    }

    pub fn entity(&self) -> &Entity<Repository> {
        &self.repository
    }

    /// The id of the repository on show.
    pub fn id<'a>(&self, cx: &'a App) -> &'a RepositoryId {
        self.repository.read(cx).id()
    }

    pub fn read<'a>(&self, cx: &'a App) -> &'a Repository {
        self.repository.read(cx)
    }

    pub fn update<R>(
        &self,
        cx: &mut App,
        update: impl FnOnce(&mut Repository, &mut Context<Repository>) -> R,
    ) -> R {
        self.repository.update(cx, update)
    }

    /// Shows the repository `id`'s, unless it's this one's; whether it was another.
    pub fn set<V: 'static>(&mut self, id: &RepositoryId, cx: &mut Context<V>) -> bool {
        if self.repository.read(cx).id() == id {
            return false;
        }
        let repository = Repositories::open(id, cx);
        *self = Shown::of(repository, self.parts, cx);
        cx.notify();
        true
    }
}

/// `limit` commits of the log after skipping `skip`, as a [`Log`] of the repository `id`.
fn log_page(git: &dyn RepositoryGit, id: &RepositoryId, skip: u32, limit: u32) -> GitTask<Log> {
    let commits = log_commits(git, skip, limit);
    let repository_id = id.to_string();
    async move {
        let commits = commits.await?;
        Ok(Log {
            repository_id,
            complete: commits.len() < limit as usize,
            commits: commits.into(),
        })
    }
    .boxed()
}

fn log_commits(git: &dyn RepositoryGit, skip: u32, limit: u32) -> GitTask<Vec<Commit>> {
    match Page::new(limit, skip) {
        Ok(page) => git.log(page),
        Err(error) => failed(git.invalid(error)),
    }
}

fn failed<T: Send + 'static>(error: DataError) -> GitTask<T> {
    async move { Err(error) }.boxed()
}

/// What `loaded` has, or why it doesn't: for one that was just loaded.
fn loaded_result<T: Clone>(loaded: Option<&Loaded<T>>) -> Result<T, DataError> {
    let loaded = loaded.ok_or_else(|| DataError::msg("It was dropped as it loaded."))?;
    match (loaded.error(), loaded.data()) {
        (Some(error), _) => Err(error.clone()),
        (None, Some(data)) => Ok(data.clone()),
        (None, None) => Err(DataError::msg("The load was cancelled.")),
    }
}

/// Every repository opened, by id: each one's [`Repository`], kept while the app runs, so what's
/// loaded of one is there when it's shown again.
#[derive(Default)]
pub struct Repositories {
    open: HashMap<RepositoryId, Entity<Repository>>,
}

impl Global for Repositories {}

impl Repositories {
    /// The repository `id`, opened with the app's git if it wasn't.
    pub fn open(id: &RepositoryId, cx: &mut App) -> Entity<Repository> {
        if let Some(repository) = cx
            .try_global::<Repositories>()
            .and_then(|repositories| repositories.open.get(id))
        {
            return repository.clone();
        }
        let git = GitRepository::new(Services::global(cx).git().clone(), id.clone());
        Repositories::insert(id, Rc::new(git), cx)
    }

    /// Opens the repository `id` with `git`, in place of one that was: for tests.
    pub fn insert(
        id: &RepositoryId,
        git: Rc<dyn RepositoryGit>,
        cx: &mut App,
    ) -> Entity<Repository> {
        let repository = cx.new(|_| Repository::new(id.clone(), git));
        cx.default_global::<Repositories>()
            .open
            .insert(id.clone(), repository.clone());
        repository
    }

    /// The repository `id`, if it's been opened.
    pub fn get(id: &str, cx: &App) -> Option<Entity<Repository>> {
        let repositories = cx.try_global::<Repositories>()?;
        repositories
            .open
            .iter()
            .find(|(open, _)| open.as_str() == id)
            .map(|(_, repository)| repository.clone())
    }

    /// Loads again what's shown of every repository opened: once git works again, say, for what
    /// failed for want of it.
    pub fn reload_all(cx: &mut App) {
        let open: Vec<_> = cx
            .try_global::<Repositories>()
            .map(|repositories| repositories.open.values().cloned().collect())
            .unwrap_or_default();
        for repository in open {
            repository.update(cx, |repository, cx| {
                drop(repository.reload_all(cx));
                if repository.overview.is_used() {
                    drop(repository.reload(Field::Overview, cx));
                }
            });
        }
    }

    /// Forgets the repository `id`, as it was removed from Gitto.
    pub fn close(id: &str, cx: &mut App) {
        if cx.has_global::<Repositories>() {
            cx.global_mut::<Repositories>()
                .open
                .retain(|open, _| open.as_str() != id);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use gitto_git::features::status::schema::{
        Head, StatusCounts, StatusSummary, Unchanged, WorkingTreeFiles,
    };
    use gpui_kit::TestAppContext;

    use super::*;

    fn repository_id() -> RepositoryId {
        RepositoryId::parse("01890a5d-ac96-774b-bcce-b302099a8057").unwrap()
    }

    fn uncommitted() -> Uncommitted {
        Uncommitted {
            head: Head::Branch {
                name: "main".into(),
                sha: "abc".into(),
            },
            upstream: None,
            ahead: 0,
            behind: 0,
            counts: StatusCounts {
                files: 1,
                staged: 0,
                unstaged: 1,
                conflicted: 0,
            },
            changes: WorkingTreeFiles {
                staged: vec![],
                unstaged: vec![
                    serde_json::from_value(serde_json::json!({
                        "path": "a.txt",
                        "status": "modified",
                        "origPath": null,
                        "additions": 1,
                        "deletions": 0,
                    }))
                    .unwrap(),
                ],
                uncounted: false,
                marker_free: vec![],
            },
            version: "v1".into(),
        }
    }

    fn commit(sha: &str) -> Commit {
        Commit {
            sha: sha.into(),
            parents: vec![],
            author_name: "Ada".into(),
            author_email: "ada@example.com".into(),
            authored_at: 0,
            committed_at: 0,
            refs: vec![],
            subject: sha.into(),
            body: String::new(),
        }
    }

    fn commits(range: std::ops::Range<usize>) -> Vec<Commit> {
        range.map(|n| commit(&format!("{n:040x}"))).collect()
    }

    /// A page of the log asked for: its skip and limit, and what answers it.
    type Asked = (u32, u32, oneshot::Sender<Vec<Commit>>);

    /// A repository's git that answers the status with `replies`, in turn, and each page of the
    /// log when the test says; it counts what it's asked.
    #[derive(Default)]
    struct Git {
        replies: RefCell<Vec<StatusReply>>,
        since: RefCell<Vec<Option<String>>>,
        pages: RefCell<Vec<Asked>>,
        refs: Cell<usize>,
        overviews: Cell<usize>,
    }

    impl RepositoryGit for Git {
        fn status(&self, since: Option<String>) -> GitTask<StatusReply> {
            self.since.borrow_mut().push(since);
            let reply = self.replies.borrow_mut().remove(0);
            async move { Ok(reply) }.boxed()
        }

        fn log(&self, page: Page) -> GitTask<Vec<Commit>> {
            let (answer, answered) = oneshot::channel();
            self.pages
                .borrow_mut()
                .push((page.skip(), page.limit(), answer));
            async move { Ok(answered.await.unwrap()) }.boxed()
        }

        fn refs(&self) -> GitTask<Vec<Ref>> {
            self.refs.set(self.refs.get() + 1);
            async { Ok(vec![]) }.boxed()
        }

        fn overview(&self) -> GitTask<Overview> {
            self.overviews.set(self.overviews.get() + 1);
            async { Err(DataError::msg("No overview.")) }.boxed()
        }
    }

    impl Git {
        /// Answers the first page asked for that's still waiting with `commits`; what it asked.
        fn answer(&self, commits: Vec<Commit>) -> (u32, u32) {
            let (skip, limit, answer) = self.pages.borrow_mut().remove(0);
            answer.send(commits).unwrap();
            (skip, limit)
        }
    }

    fn open(git: &Rc<Git>, cx: &mut TestAppContext) -> Entity<Repository> {
        let git: Rc<dyn RepositoryGit> = git.clone();
        cx.new(|_| Repository::new(repository_id(), git))
    }

    #[test]
    fn says_each_field_changed_as_a_part_of_its_own() {
        for field in Field::ALL {
            assert_eq!(field.part().field(), Some(field));
        }
        assert_eq!(Part::Patches.field(), None);
    }

    // `git/queries/status.test.ts`.

    #[gpui_kit::test]
    fn are_only_sent_again_when_they_changed(cx: &mut TestAppContext) {
        let git = Rc::new(Git::default());
        *git.replies.borrow_mut() = vec![
            StatusReply::Status(Box::new(uncommitted())),
            StatusReply::Unchanged {
                unchanged: Unchanged,
            },
        ];
        let repository = open(&git, cx);
        repository.update(cx, |repository, cx| repository.show(Field::Status, cx));
        cx.run_until_parked();
        let loaded = repository.read_with(cx, |repository, _| repository.status().data().cloned());
        let loaded = loaded.unwrap();
        assert_eq!(loaded.changes, uncommitted().changes);

        let reload = repository.update(cx, |repository, cx| repository.reload(Field::Status, cx));
        cx.run_until_parked();
        reload.into_future().now_or_never().unwrap();
        assert_eq!(*git.since.borrow(), [None, Some("v1".to_owned())]);
        let shown = repository.read_with(cx, |repository, _| repository.status().data().cloned());
        // The same files, not a copy.
        assert!(Opaque::ptr_eq(&shown.unwrap(), &loaded));
    }

    #[gpui_kit::test]
    fn are_left_out_of_the_summary(cx: &mut TestAppContext) {
        let git = Rc::new(Git::default());
        *git.replies.borrow_mut() = vec![StatusReply::Status(Box::new(uncommitted()))];
        let repository = open(&git, cx);
        repository.update(cx, |repository, cx| repository.show(Field::Status, cx));
        cx.run_until_parked();
        let summary = repository.read_with(cx, |repository, _| {
            repository.status().data().map(|status| status.summary())
        });
        assert_eq!(
            summary.unwrap(),
            StatusSummary {
                head: uncommitted().head,
                upstream: None,
                ahead: 0,
                behind: 0,
                counts: uncommitted().counts,
            }
        );
    }

    // `lib/query-client.test.ts`.

    #[gpui_kit::test]
    fn a_repositorys_data_isnt_loaded_again_when_something_else_that_shows_it_is_rendered(
        cx: &mut TestAppContext,
    ) {
        let git = Rc::new(Git::default());
        let repository = open(&git, cx);
        repository.update(cx, |repository, cx| repository.show(Field::Refs, cx));
        cx.run_until_parked();
        repository.update(cx, |repository, cx| repository.show(Field::Refs, cx));
        cx.run_until_parked();
        assert_eq!(git.refs.get(), 1);
    }

    #[gpui_kit::test]
    fn a_repositorys_data_is_loaded_again_once_its_said_to_have_changed(cx: &mut TestAppContext) {
        let git = Rc::new(Git::default());
        let repository = open(&git, cx);
        repository.update(cx, |repository, cx| drop(repository.reload_all(cx)));
        cx.run_until_parked();
        // Nothing showed it.
        assert_eq!(git.refs.get(), 0);
        repository.update(cx, |repository, cx| repository.show(Field::Refs, cx));
        cx.run_until_parked();
        repository.update(cx, |repository, cx| drop(repository.reload_all(cx)));
        cx.run_until_parked();
        assert_eq!(git.refs.get(), 2);
    }

    #[gpui_kit::test]
    fn loads_the_overview_where_its_shown_and_not_as_the_repository_changes(
        cx: &mut TestAppContext,
    ) {
        let git = Rc::new(Git::default());
        let repository = open(&git, cx);
        repository.update(cx, |repository, cx| repository.show(Field::Overview, cx));
        cx.run_until_parked();
        repository.update(cx, |repository, cx| drop(repository.reload_all(cx)));
        cx.run_until_parked();
        assert_eq!(git.overviews.get(), 1);
        let failed = repository.read_with(cx, |repository, _| {
            repository.overview().error().map(DataError::to_string)
        });
        assert_eq!(failed.as_deref(), Some("No overview."));
        // Shown again, it's out of date.
        repository.update(cx, |repository, cx| repository.show(Field::Overview, cx));
        cx.run_until_parked();
        assert_eq!(git.overviews.get(), 2);
    }

    #[gpui_kit::test]
    fn says_what_changed_and_never_notifies(cx: &mut TestAppContext) {
        let git = Rc::new(Git::default());
        let repository = open(&git, cx);
        let notified = Rc::new(Cell::new(0));
        let said = Rc::new(RefCell::new(Vec::new()));
        let _subscriptions = cx.update(|cx| {
            let (notified, said) = (Rc::clone(&notified), Rc::clone(&said));
            [
                cx.observe(&repository, move |_, _| notified.set(notified.get() + 1)),
                cx.subscribe(&repository, move |_, Changed(part), _| {
                    said.borrow_mut().push(*part)
                }),
            ]
        });
        repository.update(cx, |repository, cx| {
            drop(repository.reload(Field::Refs, cx));
            // Loading again, in place of the load under way, isn't a change.
            drop(repository.reload(Field::Refs, cx));
        });
        cx.run_until_parked();
        // As it started loading, and as it was done.
        assert_eq!(*said.borrow(), [Part::Refs, Part::Refs]);
        assert_eq!(notified.get(), 0);
    }

    // `git/queries/history.test.ts`.

    #[gpui_kit::test]
    fn loads_more_and_reloads_as_many(cx: &mut TestAppContext) {
        let git = Rc::new(Git::default());
        let repository = open(&git, cx);
        repository.update(cx, |repository, cx| repository.show(Field::Log, cx));
        cx.run_until_parked();
        assert_eq!(
            git.answer(commits(0..200)),
            (0, super::super::history::FIRST_PAGE)
        );
        cx.run_until_parked();
        assert!(repository.read_with(cx, |repository, _| repository.has_more()));

        repository.update(cx, |repository, cx| repository.load_more(cx));
        cx.run_until_parked();
        // Each commit only once: the history moved by one since.
        assert_eq!(git.answer(commits(199..1199)), (200, NEXT_PAGE));
        cx.run_until_parked();
        let log = repository.read_with(cx, |repository, _| repository.log().data().cloned());
        let log = log.unwrap();
        assert_eq!(log.commits.len(), 1199);
        assert!(!log.complete);
        assert!(!repository.read_with(cx, |repository, _| repository.is_loading_more()));

        let reload = repository.update(cx, |repository, cx| repository.reload(Field::Log, cx));
        cx.run_until_parked();
        assert_eq!(git.answer(commits(0..10)), (0, 1199));
        cx.run_until_parked();
        reload.into_future().now_or_never().unwrap();
        let log = repository.read_with(cx, |repository, _| repository.log().data().cloned());
        assert!(log.unwrap().complete);
        assert!(!repository.read_with(cx, |repository, _| repository.has_more()));
    }

    #[gpui_kit::test]
    fn drops_a_page_loaded_while_the_log_reloaded(cx: &mut TestAppContext) {
        let git = Rc::new(Git::default());
        let repository = open(&git, cx);
        repository.update(cx, |repository, cx| repository.show(Field::Log, cx));
        cx.run_until_parked();
        git.answer(commits(0..200));
        cx.run_until_parked();

        repository.update(cx, |repository, cx| repository.load_more(cx));
        cx.run_until_parked();
        let reload = repository.update(cx, |repository, cx| repository.reload(Field::Log, cx));
        cx.run_until_parked();
        // The reload's answer comes first, then the page's.
        let (_, _, page) = git.pages.borrow_mut().remove(0);
        assert_eq!(
            git.answer(commits(0..200)),
            (0, super::super::history::FIRST_PAGE)
        );
        cx.run_until_parked();
        reload.into_future().now_or_never().unwrap();
        let _ = page.send(commits(200..1200));
        cx.run_until_parked();

        let log = repository.read_with(cx, |repository, _| repository.log().data().cloned());
        assert_eq!(log.unwrap().commits.len(), 200);
        // Whether it was loading is of the log before the reload.
        assert!(!repository.read_with(cx, |repository, _| repository.is_loading_more()));
    }

    #[gpui_kit::test]
    fn takes_a_commit_from_the_log(cx: &mut TestAppContext) {
        let git = Rc::new(Git::default());
        let repository = open(&git, cx);
        let sha = format!("{:040x}", 1);
        repository.update(cx, |repository, cx| {
            repository.set_log(
                Log {
                    repository_id: repository_id().to_string(),
                    commits: commits(0..3).into(),
                    complete: true,
                },
                cx,
            );
            drop(repository.hold_commit(&sha, cx));
        });
        cx.run_until_parked();
        repository.read_with(cx, |repository, _| {
            assert_eq!(repository.commit(&sha).unwrap().sha, sha);
            assert!(repository.commit_error(&sha).is_none());
        });
    }

    // `git/queries/operation.test.ts`.

    #[gpui_kit::test]
    fn runs_one_at_a_time_per_repository_and_says_why_it_failed(cx: &mut TestAppContext) {
        let git = Rc::new(Git::default());
        let repository = open(&git, cx);
        let (finish, finished) = oneshot::channel::<Result<(), DataError>>();
        let finished = Cell::new(Some(finished));
        let pull = |repository: &mut Repository, cx: &mut Context<Repository>| {
            repository.run(
                Change::Pull,
                |_, cx| {
                    let finished = finished.take().unwrap();
                    cx.background_spawn(async move { finished.await.unwrap() })
                },
                cx,
            )
        };

        let result = repository
            .update(cx, |repository, cx| pull(repository, cx))
            .unwrap();
        cx.run_until_parked();
        assert!(repository.read_with(cx, |repository, _| {
            repository.change_state(Change::Pull).running
        }));
        // Not again while it runs.
        assert!(
            repository
                .update(cx, |repository, cx| pull(repository, cx))
                .is_none()
        );

        finish.send(Err(DataError::msg("No upstream."))).unwrap();
        cx.run_until_parked();
        let state = repository.read_with(cx, |repository, _| repository.change_state(Change::Pull));
        assert!(!state.running);
        assert_eq!(state.error.unwrap().to_string(), "No upstream.");
        assert!(result.now_or_never().unwrap().unwrap().is_err());

        repository.update(cx, |repository, cx| repository.dismiss(Change::Pull, cx));
        assert_eq!(
            repository.read_with(cx, |repository, _| repository.change_state(Change::Pull)),
            ChangeState::default()
        );
    }

    #[gpui_kit::test]
    fn forgets_one_that_went_through_and_reloads_the_repository(cx: &mut TestAppContext) {
        let git = Rc::new(Git::default());
        let repository = open(&git, cx);
        repository.update(cx, |repository, cx| repository.show(Field::Refs, cx));
        cx.run_until_parked();
        let result = repository
            .update(cx, |repository, cx| {
                repository.run(
                    Change::Push,
                    |repository, cx| repository.reloading(async { Ok(7) }.boxed(), cx),
                    cx,
                )
            })
            .unwrap();
        cx.run_until_parked();
        assert_eq!(result.now_or_never().unwrap().unwrap().unwrap(), 7);
        assert_eq!(
            repository.read_with(cx, |repository, _| repository.change_state(Change::Push)),
            ChangeState::default()
        );
        assert_eq!(git.refs.get(), 2);
    }
}
