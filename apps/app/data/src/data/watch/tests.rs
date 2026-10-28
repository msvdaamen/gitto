use std::cell::{Cell, RefCell};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use futures::stream;
use gpui_kit::{AppContext as _, Entity, TestAppContext};

use futures::FutureExt as _;
use gitto_git::features::history::commands::Page;
use gitto_git::features::history::schema::Commit;
use gitto_git::features::refs::schema::Ref;
use gitto_git::features::status::schema::{
    Head, StatusCounts, StatusReply, Uncommitted, WorkingTreeFiles,
};

use super::*;
use crate::data::git::{GitTask, RepositoryGit};
use crate::data::{Field, Repositories, Repository};

/// A watch stream whose items the test pushes; counts how many are open.
struct Procedure<T> {
    open: Arc<AtomicUsize>,
    sender: RefCell<Option<mpsc::UnboundedSender<Result<T, ApiError>>>>,
}

impl<T: Send + 'static> Procedure<T> {
    fn new() -> Self {
        Procedure {
            open: Arc::new(AtomicUsize::new(0)),
            sender: RefCell::new(None),
        }
    }

    fn open(&self) -> usize {
        self.open.load(Ordering::SeqCst)
    }

    fn call(&self) -> BoxStream<'static, Result<T, ApiError>> {
        let (sender, receiver) = mpsc::unbounded();
        *self.sender.borrow_mut() = Some(sender);
        self.open.fetch_add(1, Ordering::SeqCst);
        let open = Counted(Arc::clone(&self.open));
        // The count goes down once the stream's dropped, or ends.
        let guard = std::sync::Mutex::new(Some(open));
        receiver
            .chain(stream::poll_fn(move |_| {
                guard.lock().unwrap().take();
                std::task::Poll::Ready(None)
            }))
            .boxed()
    }

    fn push(&self, event: T) {
        if let Some(sender) = &*self.sender.borrow() {
            let _ = sender.unbounded_send(Ok(event));
        }
    }

    /// Ends the open stream with an error, like the watcher running out of file watches.
    fn fail(&self, message: &str) {
        if let Some(sender) = self.sender.borrow_mut().take() {
            let error = ApiError::from(gitto_git::Error::Bug(message.to_owned()));
            let _ = sender.unbounded_send(Err(error));
        }
    }

    /// Ends the open stream normally, like the backend closing it.
    fn end(&self) {
        self.sender.borrow_mut().take();
    }
}

/// Takes one off the count of open streams when dropped.
struct Counted(Arc<AtomicUsize>);

impl Drop for Counted {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

struct Streams {
    git_dir: Procedure<Vec<GitDirChange>>,
    working_tree: Procedure<()>,
}

impl WatchStreams for Streams {
    fn git_dir(&self, _: &RepositoryId) -> BoxStream<'static, Result<Vec<GitDirChange>, ApiError>> {
        self.git_dir.call()
    }

    fn working_tree(&self, _: &RepositoryId) -> BoxStream<'static, Result<(), ApiError>> {
        self.working_tree.call()
    }
}

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
            files: 0,
            staged: 0,
            unstaged: 0,
            conflicted: 0,
        },
        changes: WorkingTreeFiles {
            staged: vec![],
            unstaged: vec![],
            uncounted: false,
            marker_free: vec![],
        },
        version: "v1".into(),
    }
}

/// A repository's git that counts what's loaded: the status, the branches, and the log, which
/// never loads.
#[derive(Default)]
struct Git {
    status: Cell<usize>,
    refs: Cell<usize>,
    log: Cell<usize>,
}

impl RepositoryGit for Git {
    fn status(&self, _: Option<String>) -> GitTask<StatusReply> {
        self.status.set(self.status.get() + 1);
        async { Ok(StatusReply::Status(Box::new(uncommitted()))) }.boxed()
    }

    fn refs(&self) -> GitTask<Vec<Ref>> {
        self.refs.set(self.refs.get() + 1);
        async { Ok(vec![]) }.boxed()
    }

    fn log(&self, _: Page) -> GitTask<Vec<Commit>> {
        self.log.set(self.log.get() + 1);
        futures::future::pending().boxed()
    }
}

/// What the watcher loaded again of the repository.
#[derive(Debug, PartialEq, Eq)]
enum Reloaded {
    Nothing,
    /// What changes with the working tree and the index: the status.
    Uncommitted,
    /// Everything shown, the branches among it.
    All,
}

/// The repository `repository_id()`, showing its status and its branches, with `git`.
fn open(git: &Rc<Git>, cx: &mut TestAppContext) -> Entity<Repository> {
    let shared: Rc<dyn RepositoryGit> = git.clone();
    let repository = cx.update(|cx| Repositories::insert(&repository_id(), shared, cx));
    repository.update(cx, |repository, cx| {
        repository.show(Field::Status, cx);
        repository.show(Field::Refs, cx);
    });
    cx.run_until_parked();
    repository
}

struct Watched {
    git: Rc<Git>,
    streams: Rc<Streams>,
    watcher: Option<Entity<RepositoryWatcher>>,
    /// What starting to watch reloaded; the tests are about what's reloaded after.
    initial: Reloaded,
}

impl Watched {
    /// What was loaded again since it was last asked.
    fn reloaded(&self) -> Reloaded {
        let status = self.git.status.replace(0);
        let refs = self.git.refs.replace(0);
        if refs > 0 {
            Reloaded::All
        } else if status > 0 {
            Reloaded::Uncommitted
        } else {
            Reloaded::Nothing
        }
    }

    fn set_focused(&self, focused: bool, cx: &mut TestAppContext) {
        let watcher = self.watcher.clone().unwrap();
        watcher.update(cx, |watcher, cx| watcher.set_window_focused(focused, cx));
        cx.run_until_parked();
    }

    fn cleanup(&mut self, cx: &mut TestAppContext) {
        self.watcher = None;
        // Released once the app's effects are flushed, which an update does.
        cx.update(|_| {});
        cx.run_until_parked();
    }
}

fn watch(focused: bool, cx: &mut TestAppContext) -> Watched {
    let git = Rc::new(Git::default());
    let repository = open(&git, cx);
    git.status.set(0);
    git.refs.set(0);
    let streams = Rc::new(Streams {
        git_dir: Procedure::new(),
        working_tree: Procedure::new(),
    });
    let watcher = cx.new({
        let streams: Rc<dyn WatchStreams> = streams.clone();
        move |cx| RepositoryWatcher::new(&repository, streams, focused, cx)
    });
    cx.run_until_parked();
    let mut watched = Watched {
        git,
        streams,
        watcher: Some(watcher),
        initial: Reloaded::Nothing,
    };
    watched.initial = watched.reloaded();
    watched
}

#[gpui_kit::test]
fn refetches_what_was_loaded_before_it_was_watched(cx: &mut TestAppContext) {
    let mut watched = watch(true, cx);
    assert_eq!(watched.initial, Reloaded::All);
    watched.cleanup(cx);
}

#[gpui_kit::test]
fn doesnt_refetch_what_was_loaded_while_switching_to_the_repository(cx: &mut TestAppContext) {
    // The branches were loaded when the repository was last on show.
    let git = Rc::new(Git::default());
    let repository = open(&git, cx);
    // The log, loading since it came on show, before the watcher started.
    repository.update(cx, |repository, cx| repository.show(Field::Log, cx));
    cx.run_until_parked();
    assert_eq!((git.refs.get(), git.log.get()), (1, 1));

    let streams: Rc<dyn WatchStreams> = Rc::new(Streams {
        git_dir: Procedure::new(),
        working_tree: Procedure::new(),
    });
    let watcher = cx.new(|cx| RepositoryWatcher::new(&repository, streams, true, cx));
    cx.run_until_parked();
    assert_eq!((git.refs.get(), git.log.get()), (2, 1));
    drop(watcher);
}

#[gpui_kit::test]
fn doesnt_open_again_a_repository_removed_while_it_was_watched(cx: &mut TestAppContext) {
    let mut watched = watch(true, cx);
    cx.update(|cx| Repositories::close(repository_id().as_str(), cx));
    cx.run_until_parked();

    watched.streams.git_dir.push(vec![GitDirChange::Refs]);
    cx.run_until_parked();
    assert_eq!(watched.reloaded(), Reloaded::Nothing);
    assert!(
        cx.update(|cx| Repositories::get(repository_id().as_str(), cx))
            .is_none()
    );
    watched.cleanup(cx);
}

#[gpui_kit::test]
fn refetches_only_what_a_change_in_the_git_directory_affects(cx: &mut TestAppContext) {
    let mut watched = watch(true, cx);
    assert_eq!(watched.streams.git_dir.open(), 1);

    watched.streams.git_dir.push(vec![GitDirChange::Index]);
    cx.run_until_parked();
    assert_eq!(watched.reloaded(), Reloaded::Uncommitted);

    watched
        .streams
        .git_dir
        .push(vec![GitDirChange::Index, GitDirChange::Refs]);
    cx.run_until_parked();
    assert_eq!(watched.reloaded(), Reloaded::All);

    watched.cleanup(cx);
    assert_eq!(watched.streams.git_dir.open(), 0);
}

#[gpui_kit::test]
fn refetches_changes_to_the_working_tree_right_away_while_the_window_has_focus(
    cx: &mut TestAppContext,
) {
    let mut watched = watch(true, cx);
    assert_eq!(watched.streams.working_tree.open(), 1);
    assert_eq!(watched.reloaded(), Reloaded::Nothing);

    watched.streams.working_tree.push(());
    cx.run_until_parked();
    assert_eq!(watched.reloaded(), Reloaded::Uncommitted);

    watched.cleanup(cx);
    assert_eq!(watched.streams.working_tree.open(), 0);
}

#[gpui_kit::test]
fn keeps_watching_for_a_while_after_losing_focus_and_only_refetches_what_changed(
    cx: &mut TestAppContext,
) {
    let mut watched = watch(true, cx);
    assert_eq!(watched.streams.working_tree.open(), 1);

    // Nothing changed: nothing to reload, and no new watch.
    watched.set_focused(false, cx);
    watched.set_focused(true, cx);
    assert_eq!(watched.reloaded(), Reloaded::Nothing);
    assert_eq!(watched.streams.working_tree.open(), 1);

    // Changes while unfocused are reloaded once, when the window gets focus back.
    watched.set_focused(false, cx);
    watched.streams.working_tree.push(());
    cx.run_until_parked();
    watched.streams.working_tree.push(());
    cx.executor()
        .advance_clock(UNWATCH_AFTER - Duration::from_millis(1));
    cx.run_until_parked();
    assert_eq!(watched.reloaded(), Reloaded::Nothing);
    watched.set_focused(true, cx);
    assert_eq!(watched.reloaded(), Reloaded::Uncommitted);

    watched.set_focused(false, cx);
    watched.set_focused(true, cx);
    assert_eq!(watched.reloaded(), Reloaded::Nothing);

    watched.cleanup(cx);
}

#[gpui_kit::test]
fn stops_watching_once_the_window_lost_focus_long_enough_and_catches_up_after(
    cx: &mut TestAppContext,
) {
    let mut watched = watch(true, cx);
    assert_eq!(watched.streams.working_tree.open(), 1);

    watched.set_focused(false, cx);
    cx.executor().advance_clock(UNWATCH_AFTER);
    cx.run_until_parked();
    assert_eq!(watched.streams.working_tree.open(), 0);
    assert_eq!(watched.streams.git_dir.open(), 1);

    watched.set_focused(true, cx);
    assert_eq!(watched.reloaded(), Reloaded::Uncommitted);
    assert_eq!(watched.streams.working_tree.open(), 1);

    watched.cleanup(cx);
}

#[gpui_kit::test]
fn starts_watching_the_working_tree_once_the_window_gets_focus(cx: &mut TestAppContext) {
    let mut watched = watch(false, cx);
    assert_eq!(watched.streams.git_dir.open(), 1);
    assert_eq!(watched.streams.working_tree.open(), 0);

    watched.set_focused(true, cx);
    assert_eq!(watched.streams.working_tree.open(), 1);
    assert_eq!(watched.reloaded(), Reloaded::Uncommitted);

    watched.cleanup(cx);
}

#[gpui_kit::test]
fn tries_again_on_the_next_focus_when_watching_fails(cx: &mut TestAppContext) {
    let mut watched = watch(true, cx);
    assert_eq!(watched.streams.working_tree.open(), 1);

    watched.streams.working_tree.fail("no space left on device");
    cx.run_until_parked();
    assert_eq!(watched.streams.working_tree.open(), 0);

    watched.set_focused(false, cx);
    watched.set_focused(true, cx);
    assert_eq!(watched.reloaded(), Reloaded::Uncommitted);
    assert_eq!(watched.streams.working_tree.open(), 1);

    watched.cleanup(cx);
}

#[gpui_kit::test]
fn starts_watching_the_working_tree_again_on_the_next_focus_when_its_stream_ended(
    cx: &mut TestAppContext,
) {
    let mut watched = watch(true, cx);
    assert_eq!(watched.streams.working_tree.open(), 1);

    watched.streams.working_tree.end();
    cx.run_until_parked();
    watched.set_focused(false, cx);
    watched.set_focused(true, cx);
    assert_eq!(watched.reloaded(), Reloaded::Uncommitted);
    assert_eq!(watched.streams.working_tree.open(), 1);

    watched.cleanup(cx);
}

#[gpui_kit::test]
fn watches_the_git_directory_again_and_refetches_everything_when_it_failed(
    cx: &mut TestAppContext,
) {
    let mut watched = watch(true, cx);
    assert_eq!(watched.streams.git_dir.open(), 1);

    watched.streams.git_dir.fail("no space left on device");
    cx.run_until_parked();
    assert_eq!(watched.streams.git_dir.open(), 0);

    watched.set_focused(false, cx);
    watched.set_focused(true, cx);
    assert_eq!(watched.reloaded(), Reloaded::All);
    assert_eq!(watched.streams.git_dir.open(), 1);

    watched.cleanup(cx);
}
