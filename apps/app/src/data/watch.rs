//! Loading a repository's data again when it changes on disk. Ported from `git/queries/watch.ts`.
//!
//! [`RepositoryWatcher`] is `useRepositoryWatcher`: an entity the repository view makes for the
//! repository on show, and drops (stopping its watches) when it shows another. The view tells it
//! when the window gets or loses focus ([`RepositoryWatcher::set_window_focused`]).

use std::rc::Rc;
use std::time::{Duration, Instant};

use futures::StreamExt as _;
use futures::channel::mpsc;
use futures::stream::BoxStream;
use gitto_git::features::watch::schema::GitDirChange;
use gitto_git::input::RepositoryId;
use gitto_git::{ApiError, GitApi};
use gpui_kit::{AppContext as _, Context, Task};

use super::client::{QueryClient, QueryOptions, RefetchOnFocus, StaleTime};
use super::keys;
use super::repository::Repositories;

/// How long the working tree stays watched after the window loses focus. Starting to watch walks
/// the whole tree, so switching to an editor and back shouldn't start over every time.
pub const UNWATCH_AFTER: Duration = Duration::from_secs(5 * 60);

/// `options`, for a query of a repository that isn't watched, like those on the home page: loaded
/// again whenever it's shown, and when the window gets focus, rather than only when the watcher
/// says it changed (see `QueryClient::init`; `UNWATCHED`).
pub fn unwatched<T: 'static>(options: QueryOptions<T>) -> QueryOptions<T> {
    options
        .stale_time(StaleTime::Zero)
        .refetch_on_window_focus(RefetchOnFocus::IfStale)
}

/// What a repository's watches stream: git's (`rpc.git.watch`), or a test's.
pub trait WatchStreams {
    /// What changes in the git directory of the repository `id` (see `GitApi::watch_git_dir`).
    fn git_dir(&self, id: &RepositoryId)
    -> BoxStream<'static, Result<Vec<GitDirChange>, ApiError>>;
    /// An item whenever a file in the working tree of the repository `id` changes.
    fn working_tree(&self, id: &RepositoryId) -> BoxStream<'static, Result<(), ApiError>>;
}

impl WatchStreams for GitApi {
    fn git_dir(
        &self,
        id: &RepositoryId,
    ) -> BoxStream<'static, Result<Vec<GitDirChange>, ApiError>> {
        self.watch_git_dir(id)
    }

    fn working_tree(&self, id: &RepositoryId) -> BoxStream<'static, Result<(), ApiError>> {
        self.watch_working_tree(id)
    }
}

/// Loads the repository's git data again when it changes on disk; nothing else does (see
/// `QueryClient::init`). The git directory (commits, checkouts, staging) is watched all the time.
/// The working tree is watched while the window has focus and for a while after; changes to it
/// while the window doesn't have focus are loaded once it gets focus back, and not at all if there
/// weren't any. A watch that stopped (e.g. Linux ran out of file watches) is started again, with a
/// reload, when the window gets focus.
pub struct RepositoryWatcher {
    id: RepositoryId,
    streams: Rc<dyn WatchStreams>,
    focused: bool,
    git_dir: Watch,
    working_tree: Watch,
    /// Whether the working tree changed while the window didn't have focus.
    changed: bool,
    unwatch_timer: Option<Task<()>>,
}

impl RepositoryWatcher {
    /// Starts watching the repository `id`, which came on show at `shown_at` (see
    /// `QueryClient::now`), with the window focused or not.
    pub fn new(
        id: RepositoryId,
        streams: Rc<dyn WatchStreams>,
        focused: bool,
        shown_at: Instant,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut watcher = RepositoryWatcher {
            id,
            streams,
            focused,
            git_dir: Watch::new("the git directory"),
            working_tree: Watch::new("the working tree"),
            changed: false,
            unwatch_timer: None,
        };
        // Nothing watched the repository until now, so what was loaded from it before (when it was
        // last on show) may be out of date. What's loading for the first time isn't loaded twice
        // (see `QueryEntry::fetch`), nor what was loaded since it came on show.
        QueryClient::invalidate_where(
            &keys::repository(&watcher.id),
            |entry| entry.data_updated_at.is_none_or(|at| at < shown_at),
            cx,
        );
        if let Some(repository) = Repositories::get(&watcher.id, cx) {
            repository.update(cx, |repository, cx| repository.reload_when_shown(cx));
        }
        watcher.watch_git_dir(cx);
        watcher.focus_changed(None, cx);
        watcher
    }

    pub fn repository_id(&self) -> &RepositoryId {
        &self.id
    }

    /// Tells it whether the window has focus.
    pub fn set_window_focused(&mut self, focused: bool, cx: &mut Context<Self>) {
        if self.focused == focused {
            return;
        }
        let was_focused = self.focused;
        self.focused = focused;
        self.focus_changed(Some(was_focused), cx);
    }

    /// Whether each watch is running: the git directory's, and the working tree's.
    pub fn is_watching(&self) -> (bool, bool) {
        (self.git_dir.is_running(), self.working_tree.is_running())
    }

    fn focus_changed(&mut self, was_focused: Option<bool>, cx: &mut Context<Self>) {
        if !self.focused {
            self.unwatch_timer = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(UNWATCH_AFTER).await;
                let _ = this.update(cx, |watcher, _| watcher.working_tree.stop());
            }));
            return;
        }
        self.unwatch_timer = None;

        // Without a working tree watch, anything might have changed; with one, only if it did.
        let uncommitted_changed = if self.working_tree.is_running() {
            self.changed
        } else {
            was_focused == Some(false)
        };
        if !self.git_dir.is_running() {
            // A commit or a checkout might have been missed, so everything's loaded again.
            self.reload_all(cx);
            self.watch_git_dir(cx);
        } else if uncommitted_changed {
            self.reload_uncommitted(cx);
        }
        self.changed = false;
        if !self.working_tree.is_running() {
            self.watch_working_tree(cx);
        }
    }

    fn reload_all(&self, cx: &mut Context<Self>) {
        QueryClient::invalidate(&keys::repository(&self.id), cx);
        if let Some(repository) = Repositories::get(&self.id, cx) {
            repository.update(cx, |repository, cx| drop(repository.reload_all(cx)));
        }
    }

    fn reload_uncommitted(&self, cx: &mut Context<Self>) {
        QueryClient::invalidate(&keys::uncommitted(&self.id), cx);
        if let Some(repository) = Repositories::get(&self.id, cx) {
            repository.update(cx, |repository, cx| drop(repository.reload_uncommitted(cx)));
        }
    }

    fn watch_git_dir(&mut self, cx: &mut Context<Self>) {
        let stream = self.streams.git_dir(&self.id);
        let generation = self.git_dir.next_generation();
        let task = forward(
            stream,
            generation,
            |watcher| &mut watcher.git_dir,
            |watcher, changes: Vec<GitDirChange>, cx| {
                if changes.contains(&GitDirChange::Refs) {
                    watcher.reload_all(cx);
                } else {
                    watcher.reload_uncommitted(cx);
                }
            },
            cx,
        );
        self.git_dir.start(task);
    }

    fn watch_working_tree(&mut self, cx: &mut Context<Self>) {
        let stream = self.streams.working_tree(&self.id);
        let generation = self.working_tree.next_generation();
        let task = forward(
            stream,
            generation,
            |watcher| &mut watcher.working_tree,
            |watcher, (), cx| {
                if watcher.focused {
                    watcher.reload_uncommitted(cx);
                } else {
                    watcher.changed = true;
                }
            },
            cx,
        );
        self.working_tree.start(task);
    }
}

/// Calls `on_event` with everything `stream` yields, polled on the background executor, until it
/// ends, fails (which is logged) or the returned tasks are dropped; then marks the watch
/// `generation` of `watch` stopped, unless another has started since.
fn forward<T: Send + 'static>(
    mut stream: BoxStream<'static, Result<T, ApiError>>,
    generation: u64,
    watch: fn(&mut RepositoryWatcher) -> &mut Watch,
    on_event: fn(&mut RepositoryWatcher, T, &mut Context<RepositoryWatcher>),
    cx: &mut Context<RepositoryWatcher>,
) -> (Task<()>, Task<()>) {
    let (sender, mut receiver) = mpsc::unbounded();
    let pump = cx.background_spawn(async move {
        while let Some(item) = stream.next().await {
            if sender.unbounded_send(item).is_err() {
                break;
            }
        }
    });
    let consume = cx.spawn(async move |this, cx| {
        while let Some(item) = receiver.next().await {
            match item {
                Ok(event) => {
                    if this
                        .update(cx, |watcher, cx| on_event(watcher, event, cx))
                        .is_err()
                    {
                        return;
                    }
                }
                Err(error) => {
                    let what = this
                        .update(cx, |watcher, _| watch(watcher).what)
                        .unwrap_or_default();
                    log::error!("Stopped watching {what}: {error}");
                    break;
                }
            }
        }
        let _ = this.update(cx, |watcher, _| watch(watcher).ended(generation));
    });
    (pump, consume)
}

/// A watch that can be started and stopped, and knows whether it's still running.
struct Watch {
    what: &'static str,
    /// Polling the stream, and handing on what it yields: dropped to stop.
    tasks: Option<(Task<()>, Task<()>)>,
    generation: u64,
}

impl Watch {
    fn new(what: &'static str) -> Self {
        Watch {
            what,
            tasks: None,
            generation: 0,
        }
    }

    /// False once stopped, or once the stream ended by itself (an error, or the backend closing
    /// it).
    fn is_running(&self) -> bool {
        self.tasks.is_some()
    }

    fn next_generation(&mut self) -> u64 {
        self.generation += 1;
        self.generation
    }

    fn start(&mut self, tasks: (Task<()>, Task<()>)) {
        self.tasks = Some(tasks);
    }

    fn ended(&mut self, generation: u64) {
        if self.generation == generation {
            self.tasks = None;
        }
    }

    fn stop(&mut self) {
        self.tasks = None;
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use futures::stream;
    use gpui_kit::{Entity, TestAppContext};

    use super::*;
    use crate::data::key::QueryKey;

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
        fn git_dir(
            &self,
            _: &RepositoryId,
        ) -> BoxStream<'static, Result<Vec<GitDirChange>, ApiError>> {
            self.git_dir.call()
        }

        fn working_tree(&self, _: &RepositoryId) -> BoxStream<'static, Result<(), ApiError>> {
            self.working_tree.call()
        }
    }

    fn repository_id() -> RepositoryId {
        RepositoryId::parse("01890a5d-ac96-774b-bcce-b302099a8057").unwrap()
    }

    struct Watched {
        streams: Rc<Streams>,
        watcher: Option<Entity<RepositoryWatcher>>,
        /// What starting to watch reloaded; the tests are about what's reloaded after.
        initial: Vec<QueryKey>,
    }

    impl Watched {
        fn invalidated(&self, cx: &mut TestAppContext) -> Vec<QueryKey> {
            cx.update(|cx| cx.global::<QueryClient>().invalidated.clone())
        }

        fn clear(&self, cx: &mut TestAppContext) {
            cx.update(|cx| cx.global_mut::<QueryClient>().invalidated.clear());
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
        cx.update(QueryClient::init);
        let streams = Rc::new(Streams {
            git_dir: Procedure::new(),
            working_tree: Procedure::new(),
        });
        let shown_at = cx.update(|cx| QueryClient::now(cx));
        let watcher = cx.new({
            let streams: Rc<dyn WatchStreams> = streams.clone();
            move |cx| RepositoryWatcher::new(repository_id(), streams, focused, shown_at, cx)
        });
        cx.run_until_parked();
        let watched = Watched {
            streams,
            watcher: Some(watcher),
            initial: Vec::new(),
        };
        let initial = watched.invalidated(cx);
        watched.clear(cx);
        Watched { initial, ..watched }
    }

    fn id() -> String {
        repository_id().to_string()
    }

    #[gpui_kit::test]
    fn refetches_what_was_loaded_before_it_was_watched(cx: &mut TestAppContext) {
        let mut watched = watch(true, cx);
        assert_eq!(watched.initial, [keys::repository(&id())]);
        watched.cleanup(cx);
    }

    #[gpui_kit::test]
    fn doesnt_refetch_what_was_loaded_while_switching_to_the_repository(cx: &mut TestAppContext) {
        cx.update(QueryClient::init);
        // Loaded when the repository was last on show.
        cx.update(|cx| QueryClient::set_data(&keys::refs(&id()), 1, cx));
        cx.executor().advance_clock(Duration::from_secs(1));
        let shown_at = cx.update(|cx| QueryClient::now(cx));
        cx.executor().advance_clock(Duration::from_millis(100));
        // Loaded since it came on show, before the watcher started.
        cx.update(|cx| QueryClient::set_data(&keys::log(&id()), 2, cx));

        let streams: Rc<dyn WatchStreams> = Rc::new(Streams {
            git_dir: Procedure::new(),
            working_tree: Procedure::new(),
        });
        let watcher =
            cx.new(|cx| RepositoryWatcher::new(repository_id(), streams, true, shown_at, cx));
        cx.run_until_parked();

        let invalidated = |key: QueryKey, cx: &mut TestAppContext| {
            cx.update(|cx| {
                QueryClient::get_entry::<i32>(&key, cx)
                    .unwrap()
                    .read(cx)
                    .is_invalidated()
            })
        };
        assert!(invalidated(keys::refs(&id()), cx));
        assert!(!invalidated(keys::log(&id()), cx));
        drop(watcher);
    }

    #[gpui_kit::test]
    fn refetches_only_what_a_change_in_the_git_directory_affects(cx: &mut TestAppContext) {
        let mut watched = watch(true, cx);
        assert_eq!(watched.streams.git_dir.open(), 1);

        watched.streams.git_dir.push(vec![GitDirChange::Index]);
        cx.run_until_parked();
        assert_eq!(watched.invalidated(cx), [keys::uncommitted(&id())]);

        watched.clear(cx);
        watched
            .streams
            .git_dir
            .push(vec![GitDirChange::Index, GitDirChange::Refs]);
        cx.run_until_parked();
        assert_eq!(watched.invalidated(cx), [keys::repository(&id())]);

        watched.cleanup(cx);
        assert_eq!(watched.streams.git_dir.open(), 0);
    }

    #[gpui_kit::test]
    fn refetches_changes_to_the_working_tree_right_away_while_the_window_has_focus(
        cx: &mut TestAppContext,
    ) {
        let mut watched = watch(true, cx);
        assert_eq!(watched.streams.working_tree.open(), 1);
        assert_eq!(watched.invalidated(cx), []);

        watched.streams.working_tree.push(());
        cx.run_until_parked();
        assert_eq!(watched.invalidated(cx), [keys::uncommitted(&id())]);

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
        assert_eq!(watched.invalidated(cx), []);
        assert_eq!(watched.streams.working_tree.open(), 1);

        // Changes while unfocused are reloaded once, when the window gets focus back.
        watched.set_focused(false, cx);
        watched.streams.working_tree.push(());
        cx.run_until_parked();
        watched.streams.working_tree.push(());
        cx.executor()
            .advance_clock(UNWATCH_AFTER - Duration::from_millis(1));
        cx.run_until_parked();
        assert_eq!(watched.invalidated(cx), []);
        watched.set_focused(true, cx);
        assert_eq!(watched.invalidated(cx), [keys::uncommitted(&id())]);

        watched.clear(cx);
        watched.set_focused(false, cx);
        watched.set_focused(true, cx);
        assert_eq!(watched.invalidated(cx), []);

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
        assert_eq!(watched.invalidated(cx), [keys::uncommitted(&id())]);
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
        assert_eq!(watched.invalidated(cx), [keys::uncommitted(&id())]);

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
        assert_eq!(watched.invalidated(cx), [keys::uncommitted(&id())]);
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
        assert_eq!(watched.invalidated(cx), [keys::uncommitted(&id())]);
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
        assert_eq!(watched.invalidated(cx), [keys::repository(&id())]);
        assert_eq!(watched.streams.git_dir.open(), 1);

        watched.cleanup(cx);
    }
}
