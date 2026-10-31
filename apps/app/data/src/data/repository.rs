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
//! [`RepositoryGit`]; loading something again as it loads loads it once more once that's done.
//!
//! What's loaded is kept up to date by `super::watch::RepositoryWatcher`, which reloads it when the
//! repository changes on disk (see [`Repository::reload_all`]), and nothing else does: not focus,
//! nor a timer. What never changes, a commit and what's in it, is kept, a few at a time, for
//! whichever view shows it next.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use futures::FutureExt as _;
use futures::channel::oneshot;
use futures::future::{BoxFuture, join_all};
use gitto_git::Page;
use gitto_git::features::conflicts::schema::Conflict;
use gitto_git::features::diff::schema::ChangedFile;
use gitto_git::features::history::schema::Commit;
use gitto_git::features::operation::schema::Operation;
use gitto_git::features::overview::schema::Overview;
use gitto_git::features::refs::schema::Ref;
use gitto_git::features::remote::schema::PushTarget;
use gitto_git::features::stash::schema::Stash;
use gitto_git::features::status::schema::Uncommitted;
use gitto_git::features::worktree::schema::Worktree;
use gitto_git::input::RepositoryId;
use gpui_kit::{Context, EventEmitter, Task};

use super::changes::{Change, ChangeGroup, Done};
use super::diff::{ChangedFiles, FilesOf};
use super::error::DataError;
use super::file_diff::FilePatch;
use super::git::{GitTask, RepositoryGit};
use super::history::Log;
use super::keyed::Keyed;
use super::loaded::Loaded;
use super::opaque::Opaque;
use gitto_ui_logic::git::diff_source::{DiffFileKey, DiffSource};

mod keyed_loads;
mod loading;
mod registry;

pub use self::registry::{Repositories, Shown, observe_parts, observe_selected};

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
type PatchKey = DiffFileKey;

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
    /// Each with the file it's of, as the view last asked for it: its status and the path it was
    /// renamed from say how git reads it.
    patches: Keyed<PatchKey, FilePatch, ChangedFile>,
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
    /// What a test gave it (see [`is_seeded`](Self::is_seeded)).
    #[cfg(any(test, feature = "test-support"))]
    seeded: std::collections::HashSet<Part>,
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
            #[cfg(any(test, feature = "test-support"))]
            seeded: Default::default(),
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
    #[cfg(any(test, feature = "test-support"))]
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

    // Setting what's loaded: tests' data.

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_status(&mut self, status: Uncommitted, cx: &mut Context<Self>) {
        self.status.cancel();
        self.status.set(Opaque::new(status));
        self.seed(Part::Status, cx);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_operation(&mut self, operation: Option<Operation>, cx: &mut Context<Self>) {
        self.operation.cancel();
        self.operation.set(operation);
        self.seed(Part::Operation, cx);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_log(&mut self, log: Log, cx: &mut Context<Self>) {
        self.log.cancel();
        self.log.set(log);
        self.seed(Part::Log, cx);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_refs(&mut self, refs: Vec<Ref>, cx: &mut Context<Self>) {
        self.refs.cancel();
        self.refs.set(refs.into());
        self.seed(Part::Refs, cx);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_remotes(&mut self, remotes: Vec<String>, cx: &mut Context<Self>) {
        self.remotes.cancel();
        self.remotes.set(remotes);
        self.seed(Part::Remotes, cx);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_stashes(&mut self, stashes: Vec<Stash>, cx: &mut Context<Self>) {
        self.stashes.cancel();
        self.stashes.set(stashes.into());
        self.seed(Part::Stashes, cx);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_worktrees(&mut self, worktrees: Vec<Worktree>, cx: &mut Context<Self>) {
        self.worktrees.cancel();
        self.worktrees.set(worktrees.into());
        self.seed(Part::Worktrees, cx);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_push_target(&mut self, target: PushTarget, cx: &mut Context<Self>) {
        self.push_target.cancel();
        self.push_target.set(target);
        self.seed(Part::PushTarget, cx);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_pushed_to(&mut self, sha: &str, remote: Option<String>, cx: &mut Context<Self>) {
        self.pushed_to.set(sha.to_owned(), remote);
        self.seed(Part::PushedTo, cx);
    }

    /// The version of the patch of the file at `path` in `source`, if it's there (see
    /// [`replace_patch`](Self::replace_patch)).
    pub(crate) fn patch_version(&self, source: &DiffSource, path: &str) -> Option<(u64, u64)> {
        let loaded = self.patches.get(&DiffFileKey::new(source, path))?;
        Some(loaded.version())
    }

    /// Puts `patch` in place of the patch of the file at `path` in `source`, and of a load of it
    /// under way, as of the file as it was last asked for; whether there was one to replace. Not if
    /// that went out of date, or started loading, since it was at `version` (see
    /// `Loaded::version`): what loads then may be newer than `patch`.
    pub(crate) fn replace_patch(
        &mut self,
        source: &DiffSource,
        path: &str,
        patch: &str,
        version: Option<(u64, u64)>,
    ) -> bool {
        let key = DiffFileKey::new(source, path);
        let file = self.patches.asked(&key).cloned();
        let Some(loaded) = self.patches.get_mut(&key) else {
            return false;
        };
        let Some(last) = loaded.data().cloned() else {
            return false;
        };
        if version != Some(loaded.version()) {
            return false;
        }
        loaded.cancel();
        loaded.set(FilePatch {
            file: file.unwrap_or(last.file),
            patch: Arc::from(patch),
            ..last
        });
        true
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_patch(&mut self, source: &DiffSource, patch: FilePatch, cx: &mut Context<Self>) {
        let key = DiffFileKey::new(source, &patch.file.path);
        self.patches.ask(key.clone(), patch.file.clone());
        self.patches.set(key, patch);
        self.changed(Part::Patches, cx);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_conflict(&mut self, path: &str, conflict: Conflict, cx: &mut Context<Self>) {
        self.conflicts.set(path.to_owned(), conflict);
        self.changed(Part::Conflicts, cx);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_files(&mut self, of: FilesOf, files: ChangedFiles, cx: &mut Context<Self>) {
        self.files.set((of, files.sha.clone()), files);
        self.changed(Part::Commits, cx);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_message(&mut self, sha: &str, message: String, cx: &mut Context<Self>) {
        self.messages.set(sha.to_owned(), message);
        self.changed(Part::Commits, cx);
    }

    /// Says `part` changed (see the module's doc).
    fn changed(&mut self, part: Part, cx: &mut Context<Self>) {
        cx.emit(Changed(part));
    }

    /// Says `part` changed, as a test gave it: what it gave stays, as git isn't asked for it again
    /// (see [`is_seeded`](Self::is_seeded)).
    #[cfg(any(test, feature = "test-support"))]
    fn seed(&mut self, part: Part, cx: &mut Context<Self>) {
        self.seeded.insert(part);
        self.changed(part, cx);
    }

    /// Whether a test gave it `part`, a field or where commits were pushed (`set_*`): that's
    /// reloaded as it is, without asking git, so a test's git only answers what the test is about,
    /// and a change the test makes reloads what it gave as git would, unchanged.
    fn is_seeded(&self, part: Part) -> bool {
        #[cfg(any(test, feature = "test-support"))]
        return self.seeded.contains(&part);
        #[cfg(not(any(test, feature = "test-support")))]
        {
            let _ = part;
            false
        }
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

    /// Runs `change` with `input`, as a view made it from what git said or what was typed, as
    /// `then` does, if it's valid. If it isn't, that's a bug (see `RepositoryGit::invalid`): the
    /// change doesn't run (`None`), and shows as failing, unless it's running.
    pub fn parsing<T, R>(
        &mut self,
        change: Change,
        input: gitto_git::Result<T>,
        then: impl FnOnce(&mut Self, T, &mut Context<Self>) -> Done<R>,
        cx: &mut Context<Self>,
    ) -> Done<R> {
        let error = match input {
            Ok(input) => return then(self, input, cx),
            Err(error) => self.git.invalid(error),
        };
        if !self.change_state(change).running {
            self.changes.insert(
                change,
                ChangeState {
                    running: false,
                    error: Some(error),
                },
            );
            self.changed(Part::Changes, cx);
        }
        None
    }
}

/// `limit` commits of the log after skipping `skip`, as a [`Log`] of the repository `id`.
fn log_page(git: &dyn RepositoryGit, id: &RepositoryId, skip: u32, limit: u32) -> GitTask<Log> {
    let commits = log_commits(git, skip, limit);
    let repository_id = id.clone();
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

/// Why a change to a repository that was removed from Gitto failed.
pub(crate) fn removed() -> DataError {
    DataError::msg("The repository was removed from Gitto.")
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

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use gitto_git::PatchVersion;
    use gitto_git::features::staging::schema::LineSelection;
    use gitto_git::features::status::schema::{
        Head, StatusCounts, StatusReply, StatusSummary, Unchanged, WorkingTreeFiles,
    };
    use gitto_git::input::FileInput;
    use gpui_kit::{AppContext as _, Entity, TestAppContext};

    use super::*;
    use crate::data::changes::{Scope, Then};
    use crate::data::history::NEXT_PAGE;
    use crate::data::staging::StageAction;

    fn repository_id() -> RepositoryId {
        RepositoryId::parse("01890a5d-ac96-774b-bcce-b302099a8057").unwrap()
    }

    fn uncommitted() -> Uncommitted {
        Uncommitted {
            summary: StatusSummary {
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
    type Asked = (u32, u32, oneshot::Sender<Result<Vec<Commit>, DataError>>);

    /// A repository's git that answers the status with `replies`, in turn, and each page of the
    /// log when the test says; it counts what it's asked.
    #[derive(Default)]
    struct Git {
        replies: RefCell<Vec<StatusReply>>,
        since: RefCell<Vec<Option<gitto_git::ContentVersion>>>,
        pages: RefCell<Vec<Asked>>,
        refs: Cell<usize>,
        overviews: Cell<usize>,
    }

    impl RepositoryGit for Git {
        fn status(&self, since: Option<gitto_git::ContentVersion>) -> GitTask<StatusReply> {
            self.since.borrow_mut().push(since);
            let reply = self.replies.borrow_mut().remove(0);
            async move { Ok(reply) }.boxed()
        }

        fn log(&self, page: Page) -> GitTask<Vec<Commit>> {
            let (answer, answered) = oneshot::channel();
            self.pages
                .borrow_mut()
                .push((page.skip(), page.limit(), answer));
            async move { answered.await.unwrap() }.boxed()
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
            self.answer_with(Ok(commits))
        }

        /// Answers the first page asked for that's still waiting with `answer`; what it asked.
        fn answer_with(&self, answer: Result<Vec<Commit>, DataError>) -> (u32, u32) {
            let (skip, limit, answered) = self.pages.borrow_mut().remove(0);
            answered.send(answer).unwrap();
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
        assert_eq!(
            *git.since.borrow(),
            [None, Some(gitto_git::ContentVersion::from("v1"))]
        );
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
            repository
                .status()
                .data()
                .map(|status| status.summary.clone())
        });
        assert_eq!(summary.unwrap(), uncommitted().summary);
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
            // Loading again as it loads, once it's done, isn't a change yet.
            drop(repository.reload(Field::Refs, cx));
            drop(repository.reload(Field::Refs, cx));
        });
        assert_eq!(*said.borrow(), [Part::Refs]);
        cx.run_until_parked();
        // As it started loading, as that was done and it loaded again, and as that was done.
        assert_eq!(*said.borrow(), [Part::Refs, Part::Refs, Part::Refs]);
        assert_eq!(git.refs.get(), 2);
        assert_eq!(notified.get(), 0);
    }

    /// A repository's git that answers each status asked for when the test says, and stages all
    /// at once.
    #[derive(Default)]
    struct Statuses(RefCell<Vec<oneshot::Sender<StatusReply>>>);

    impl RepositoryGit for Statuses {
        fn status(&self, _: Option<gitto_git::ContentVersion>) -> GitTask<StatusReply> {
            let (answer, answered) = oneshot::channel();
            self.0.borrow_mut().push(answer);
            async move { answered.await.map_err(|_| DataError::msg("Not answered.")) }.boxed()
        }

        fn stage_all(&self) -> GitTask<()> {
            async { Ok(()) }.boxed()
        }
    }

    impl Statuses {
        /// How many statuses are waiting for an answer.
        fn waiting(&self) -> usize {
            self.0.borrow().len()
        }

        /// Answers the first status asked for that's waiting, at `version`.
        fn answer(&self, version: &str) {
            let mut status = uncommitted();
            status.version = version.into();
            let reply = StatusReply::Status(Box::new(status));
            assert!(self.0.borrow_mut().remove(0).send(reply).is_ok());
        }
    }

    fn version(repository: &Entity<Repository>, cx: &mut TestAppContext) -> Option<String> {
        repository.read_with(cx, |repository, _| {
            let status = repository.status().data()?;
            Some(status.version.to_string())
        })
    }

    #[gpui_kit::test]
    fn loads_once_more_after_the_load_under_way_rather_than_cancelling_it(cx: &mut TestAppContext) {
        let git = Rc::new(Statuses::default());
        let shared: Rc<dyn RepositoryGit> = git.clone();
        let repository = cx.new(|_| Repository::new(repository_id(), shared));
        repository.update(cx, |repository, cx| repository.show(Field::Status, cx));
        cx.run_until_parked();
        // Files keep changing as git reads the status, as a build's can.
        for _ in 0..3 {
            repository.update(cx, |repository, cx| drop(repository.reload_uncommitted(cx)));
            cx.run_until_parked();
        }
        assert_eq!(git.waiting(), 1);

        git.answer("v1");
        cx.run_until_parked();
        // What it read is shown, and read again, as it may be out of date.
        assert_eq!(version(&repository, cx).as_deref(), Some("v1"));
        assert_eq!(git.waiting(), 1);
        git.answer("v2");
        cx.run_until_parked();
        assert_eq!(version(&repository, cx).as_deref(), Some("v2"));
        assert_eq!(git.waiting(), 0);
        repository.read_with(cx, |repository, _| {
            assert!(!repository.status().is_fetching() && !repository.status().is_stale());
        });
    }

    #[gpui_kit::test]
    fn a_change_that_waits_for_its_reload_waits_for_the_load_after_the_one_under_way(
        cx: &mut TestAppContext,
    ) {
        let git = Rc::new(Statuses::default());
        let shared: Rc<dyn RepositoryGit> = git.clone();
        let repository = cx.new(|_| Repository::new(repository_id(), shared));
        repository.update(cx, |repository, cx| repository.show(Field::Status, cx));
        cx.run_until_parked();

        let done = Rc::new(Cell::new(false));
        let staging = repository.update(cx, |repository, cx| {
            repository.stage(crate::data::staging::StagingTarget::All, cx)
        });
        cx.update(|cx| {
            let done = Rc::clone(&done);
            cx.spawn(async move |_| {
                staging.await.unwrap();
                done.set(true);
            })
            .detach();
        });
        cx.run_until_parked();
        // The status read before staging, then the one read after it.
        git.answer("before");
        cx.run_until_parked();
        assert!(!done.get());
        git.answer("after");
        cx.run_until_parked();
        assert!(done.get());
        assert_eq!(version(&repository, cx).as_deref(), Some("after"));
    }

    #[gpui_kit::test]
    fn loads_again_a_patch_read_before_the_uncommitted_changes_changed(cx: &mut TestAppContext) {
        /// Answers each patch asked for when the test says.
        #[derive(Default)]
        struct Patches(RefCell<Vec<oneshot::Sender<String>>>);

        impl RepositoryGit for Patches {
            fn unstaged_file_patch(&self, _: FileInput, _: bool) -> GitTask<String> {
                let (answer, answered) = oneshot::channel();
                self.0.borrow_mut().push(answer);
                async move { Ok(answered.await.unwrap()) }.boxed()
            }
        }

        let git = Rc::new(Patches::default());
        let shared: Rc<dyn RepositoryGit> = git.clone();
        let repository = cx.new(|_| Repository::new(repository_id(), shared));
        let file = uncommitted().changes.unstaged[0].clone();
        // Loaded ahead, as the pointer rests on the file.
        drop(repository.update(cx, |repository, cx| {
            repository.fetch_patch(&DiffSource::Unstaged, &file, cx)
        }));
        cx.run_until_parked();
        // The file changes as it loads.
        repository.update(cx, |repository, cx| drop(repository.reload_uncommitted(cx)));
        git.0.borrow_mut().remove(0).send("before".into()).unwrap();
        cx.run_until_parked();

        let opened = repository.update(cx, |repository, cx| {
            repository.fetch_patch(&DiffSource::Unstaged, &file, cx)
        });
        cx.run_until_parked();
        git.0.borrow_mut().remove(0).send("after".into()).unwrap();
        cx.run_until_parked();
        let patch = opened.now_or_never().unwrap().unwrap();
        assert_eq!(&*patch.patch, "after");
    }

    #[gpui_kit::test]
    fn loads_a_held_patch_as_git_reads_the_file_now_once_its_status_changed(
        cx: &mut TestAppContext,
    ) {
        /// Answers each unstaged patch at once, and stages lines; says whether it was asked for
        /// each as of an untracked file.
        #[derive(Default)]
        struct Patches(RefCell<Vec<bool>>);

        impl RepositoryGit for Patches {
            fn unstaged_file_patch(&self, _: FileInput, untracked: bool) -> GitTask<String> {
                self.0.borrow_mut().push(untracked);
                async { Ok("diff\n@@ -1 +1 @@\n-a\n+b\n".to_owned()) }.boxed()
            }

            fn stage_lines(
                &self,
                _: FileInput,
                untracked: bool,
                _: PatchVersion,
                _: LineSelection,
            ) -> GitTask<String> {
                self.0.borrow_mut().push(untracked);
                async { Ok("diff\n@@ -1 +1 @@\n-a\n+c\n".to_owned()) }.boxed()
            }
        }

        let git = Rc::new(Patches::default());
        let shared: Rc<dyn RepositoryGit> = git.clone();
        let repository = cx.new(|_| Repository::new(repository_id(), shared));
        let file = |status: &str| -> ChangedFile {
            serde_json::from_value(serde_json::json!({
                "path": "a.txt",
                "status": status,
                "origPath": null,
                "additions": 1,
                "deletions": 0,
            }))
            .unwrap()
        };
        let _untracked = repository.update(cx, |repository, cx| {
            repository.hold_patch(&DiffSource::Unstaged, &file("untracked"), cx)
        });
        cx.run_until_parked();
        // Some of its lines were staged: it's in the index, and its list says it's modified.
        let _modified = repository.update(cx, |repository, cx| {
            repository.hold_patch(&DiffSource::Unstaged, &file("modified"), cx)
        });
        cx.run_until_parked();
        repository.update(cx, |repository, cx| drop(repository.reload_uncommitted(cx)));
        cx.run_until_parked();
        assert_eq!(*git.0.borrow(), [true, false, false]);

        let staged = repository.update(cx, |repository, cx| {
            let patch = repository.patch(&DiffSource::Unstaged, "a.txt").unwrap();
            let version = PatchVersion::of(&patch.data().unwrap().patch);
            let lines = LineSelection {
                deletions: vec![],
                additions: vec![],
            };
            repository.stage_lines(StageAction::Stage, file("modified"), version, lines, cx)
        });
        cx.run_until_parked();
        assert!(staged.now_or_never().unwrap().is_ok());
        assert_eq!(*git.0.borrow(), [true, false, false, false]);
        let shown = repository.read_with(cx, |repository, _| {
            let patch = repository.patch(&DiffSource::Unstaged, "a.txt").unwrap();
            patch.data().cloned().unwrap()
        });
        assert!(shown.patch.ends_with("+c\n"));
        assert_eq!(shown.file, file("modified"));
    }

    #[gpui_kit::test]
    fn keeps_a_patch_loaded_as_lines_were_staged_rather_than_what_they_left(
        cx: &mut TestAppContext,
    ) {
        /// Answers each patch, and the staging of lines, when the test says.
        #[derive(Default)]
        struct Gated {
            patches: RefCell<Vec<oneshot::Sender<String>>>,
            staging: RefCell<Option<oneshot::Sender<String>>>,
        }

        impl RepositoryGit for Gated {
            fn unstaged_file_patch(&self, _: FileInput, _: bool) -> GitTask<String> {
                let (answer, answered) = oneshot::channel();
                self.patches.borrow_mut().push(answer);
                async move { answered.await.map_err(|_| DataError::msg("Cancelled.")) }.boxed()
            }

            fn stage_lines(
                &self,
                _: FileInput,
                _: bool,
                _: PatchVersion,
                _: LineSelection,
            ) -> GitTask<String> {
                let (answer, answered) = oneshot::channel();
                *self.staging.borrow_mut() = Some(answer);
                async move { answered.await.map_err(|_| DataError::msg("Cancelled.")) }.boxed()
            }
        }

        impl Gated {
            fn answer(&self, patch: &str) {
                let answer = self.patches.borrow_mut().remove(0);
                assert!(answer.send(format!("diff\n@@ -1 +1 @@\n{patch}\n")).is_ok());
            }
        }

        let git = Rc::new(Gated::default());
        let shared: Rc<dyn RepositoryGit> = git.clone();
        let repository = cx.new(|_| Repository::new(repository_id(), shared));
        let file = uncommitted().changes.unstaged[0].clone();
        let _hold = repository.update(cx, |repository, cx| {
            repository.hold_patch(&DiffSource::Unstaged, &file, cx)
        });
        cx.run_until_parked();
        git.answer("+picked");
        cx.run_until_parked();

        let lines = LineSelection {
            deletions: vec![],
            additions: vec![],
        };
        let staged = repository.update(cx, |repository, cx| {
            let version = PatchVersion::of("picked");
            repository.stage_lines(StageAction::Stage, file.clone(), version, lines, cx)
        });
        cx.run_until_parked();
        // An editor saves the file as the lines are staged.
        repository.update(cx, |repository, cx| drop(repository.reload_uncommitted(cx)));
        cx.run_until_parked();
        let staging = git.staging.borrow_mut().take().unwrap();
        staging.send("diff\n@@ -1 +1 @@\n+left\n".into()).unwrap();
        cx.run_until_parked();
        assert!(staged.now_or_never().unwrap().is_ok());
        // The load after the save goes on, and is loaded again after, as staging changed it too.
        git.answer("+saved");
        cx.run_until_parked();
        git.answer("+saved and staged");
        cx.run_until_parked();
        let shown = repository.read_with(cx, |repository, _| {
            let patch = repository.patch(&DiffSource::Unstaged, &file.path).unwrap();
            patch.data().unwrap().patch.clone()
        });
        assert!(shown.ends_with("+saved and staged\n"), "{shown}");
    }

    #[gpui_kit::test]
    fn says_patches_changed_only_when_one_on_show_did(cx: &mut TestAppContext) {
        let git = Rc::new(Statuses::default());
        let shared: Rc<dyn RepositoryGit> = git.clone();
        let repository = cx.new(|_| Repository::new(repository_id(), shared));
        repository.update(cx, |repository, cx| repository.show(Field::Status, cx));
        git.answer("v1");
        cx.run_until_parked();
        let said = Rc::new(RefCell::new(Vec::new()));
        let _subscription = cx.update(|cx| {
            let said = Rc::clone(&said);
            cx.subscribe(&repository, move |_, Changed(part), _| {
                said.borrow_mut().push(*part)
            })
        });
        repository.update(cx, |repository, cx| drop(repository.reload_uncommitted(cx)));
        git.answer("v2");
        cx.run_until_parked();
        assert_eq!(*said.borrow(), [Part::Status, Part::Status]);
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
    fn reloads_the_log_from_its_first_page_when_its_order_changed(cx: &mut TestAppContext) {
        let git = Rc::new(Git::default());
        let repository = open(&git, cx);
        repository.update(cx, |repository, cx| repository.show(Field::Log, cx));
        cx.run_until_parked();
        git.answer(commits(0..200));
        cx.run_until_parked();

        repository.update(cx, |repository, cx| repository.load_more(cx));
        cx.run_until_parked();
        let changed = gitto_git::ApiError::unreported(gitto_git::Error::LogOrderChanged);
        assert_eq!(git.answer_with(Err(changed.into())), (200, NEXT_PAGE));
        cx.run_until_parked();
        // Read again from the top, keeping its place, rather than saying more couldn't load.
        assert!(repository.read_with(cx, |repository, _| repository.more_error().is_none()));
        assert_eq!(
            git.answer(commits(0..200)),
            (0, super::super::history::FIRST_PAGE)
        );
        cx.run_until_parked();
        let log = repository.read_with(cx, |repository, _| repository.log().data().cloned());
        assert_eq!(log.unwrap().commits.len(), 200);
        // And more of it loads as before, now that it's sorted.
        repository.update(cx, |repository, cx| repository.load_more(cx));
        cx.run_until_parked();
        assert_eq!(git.answer(commits(200..1200)), (200, NEXT_PAGE));
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
        let _ = page.send(Ok(commits(200..1200)));
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
                    repository_id: repository_id(),
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

    /// A repository's git that can't read commits until the test says it can; counts what it's
    /// asked.
    #[derive(Default)]
    struct Unreachable {
        works: Cell<bool>,
        commits: Cell<usize>,
    }

    impl RepositoryGit for Unreachable {
        fn commit(&self, sha: gitto_git::input::Sha) -> GitTask<Commit> {
            self.commits.set(self.commits.get() + 1);
            let result = if self.works.get() {
                Ok(commit(sha.as_str()))
            } else {
                Err(DataError::msg("The share is unreachable."))
            };
            async move { result }.boxed()
        }
    }

    #[gpui_kit::test]
    fn loads_a_commit_again_once_its_shown_again_after_failing(cx: &mut TestAppContext) {
        let git = Rc::new(Unreachable::default());
        let shared: Rc<dyn RepositoryGit> = git.clone();
        let repository = cx.new(|_| Repository::new(repository_id(), shared));
        let sha = format!("{:040x}", 1);
        let hold = repository.update(cx, |repository, cx| repository.hold_commit(&sha, cx));
        cx.run_until_parked();
        assert!(repository.read_with(cx, |repository, _| repository.commit_error(&sha).is_some()));
        drop(hold);

        git.works.set(true);
        let _hold = repository.update(cx, |repository, cx| repository.hold_commit(&sha, cx));
        cx.run_until_parked();
        assert_eq!(git.commits.get(), 2);
        repository.read_with(cx, |repository, _| {
            assert_eq!(repository.commit(&sha).unwrap().sha, sha);
            assert!(repository.commit_error(&sha).is_none());
        });
    }

    #[gpui_kit::test]
    fn loads_again_what_failed_of_every_repository_once_git_works_again(cx: &mut TestAppContext) {
        let git = Rc::new(Unreachable::default());
        let shared: Rc<dyn RepositoryGit> = git.clone();
        let repository = cx.update(|cx| Repositories::insert(&repository_id(), shared, cx));
        let sha = format!("{:040x}", 1);
        let _hold = repository.update(cx, |repository, cx| repository.hold_commit(&sha, cx));
        cx.run_until_parked();

        git.works.set(true);
        cx.update(Repositories::reload_all);
        cx.run_until_parked();
        assert_eq!(git.commits.get(), 2);
        assert!(repository.read_with(cx, |repository, _| repository.commit_error(&sha).is_none()));
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
                    |repository, cx| {
                        repository.after(
                            async { Ok(7) }.boxed(),
                            |_| Then::AwaitReload(Scope::All),
                            cx,
                        )
                    },
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

    #[gpui_kit::test]
    fn doesnt_open_again_a_repository_removed_before_a_change_to_it(cx: &mut TestAppContext) {
        let git: Rc<dyn RepositoryGit> = Rc::new(Git::default());
        cx.update(|cx| Repositories::insert(&repository_id(), git, cx));
        cx.update(|cx| Repositories::close(&repository_id(), cx));
        let changed = cx.update(|cx| {
            Repositories::update_opened(
                &repository_id(),
                |_, _| Task::ready(Ok::<_, DataError>(())),
                cx,
            )
        });
        let error = changed.now_or_never().unwrap().unwrap_err();
        assert_eq!(error.to_string(), "The repository was removed from Gitto.");
        assert!(
            cx.update(|cx| Repositories::get(&repository_id(), cx))
                .is_none()
        );
    }

    #[gpui_kit::test]
    fn doesnt_open_again_a_repository_removed_from_gitto(cx: &mut TestAppContext) {
        let git: Rc<dyn RepositoryGit> = Rc::new(Git::default());
        cx.update(|cx| Repositories::insert(&repository_id(), git, cx));
        cx.update(|cx| Repositories::close(&repository_id(), cx));
        // A view about to go with it, say, or a save done after it was removed.
        let repository = cx.update(|cx| Repositories::open(&repository_id(), cx));
        assert!(
            cx.update(|cx| Repositories::get(&repository_id(), cx))
                .is_none()
        );
        repository.update(cx, |repository, cx| repository.show(Field::Refs, cx));
        cx.run_until_parked();
        let error = repository.read_with(cx, |repository, _| repository.refs().error().cloned());
        assert_eq!(
            error.unwrap().to_string(),
            "The repository was removed from Gitto."
        );
    }

    #[gpui_kit::test]
    fn settles_a_load_of_a_repository_removed_as_it_loaded(cx: &mut TestAppContext) {
        let git = Rc::new(Statuses::default());
        let shared: Rc<dyn RepositoryGit> = git.clone();
        let repository = cx.update(|cx| Repositories::insert(&repository_id(), shared, cx));
        repository.update(cx, |repository, cx| repository.show(Field::Status, cx));
        cx.run_until_parked();
        cx.update(|cx| Repositories::close(&repository_id(), cx));
        git.answer("v1");
        cx.run_until_parked();
        // What still shows it has it, and it isn't opened again.
        assert_eq!(version(&repository, cx).as_deref(), Some("v1"));
        assert!(
            cx.update(|cx| Repositories::get(&repository_id(), cx))
                .is_none()
        );
    }

    #[gpui_kit::test]
    fn forgets_what_only_their_page_shows_of_repositories_off_show_long_ago(
        cx: &mut TestAppContext,
    ) {
        let ids: Vec<RepositoryId> = (0..5).map(|_| RepositoryId::generate()).collect();
        for id in &ids {
            let git: Rc<dyn RepositoryGit> = Rc::new(Git::default());
            let repository = cx.update(|cx| Repositories::insert(id, git, cx));
            repository.update(cx, |repository, cx| {
                repository.set_status(uncommitted(), cx);
                repository.set_refs(vec![], cx);
            });
        }
        let kept = |id: &RepositoryId, cx: &mut TestAppContext| {
            let repository = cx.update(|cx| Repositories::get(id, cx).unwrap());
            repository.read_with(cx, |repository, _| {
                assert!(repository.status().data().is_some());
                repository.refs().data().is_some()
            })
        };
        // Shown in turn, the last one again.
        for id in &ids[..4] {
            cx.update(|cx| {
                Repositories::came_on_show(id, cx);
                Repositories::went_off_show(id, cx);
            });
        }
        cx.update(|cx| Repositories::came_on_show(&ids[1], cx));
        cx.update(|cx| Repositories::went_off_show(&ids[1], cx));
        let kept: Vec<bool> = ids.iter().map(|id| kept(id, cx)).collect();
        assert_eq!(kept, [false, true, true, true, true]);
    }
}
