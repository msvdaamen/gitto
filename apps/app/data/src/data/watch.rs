//! Loading a repository's data again when it changes on disk. Ported from `git/queries/watch.ts`.
//!
//! [`RepositoryWatcher`] is `useRepositoryWatcher`: an entity the repository view makes for the
//! repository on show, and drops (stopping its watches) when it shows another. The view tells it
//! when the window gets or loses focus ([`RepositoryWatcher::set_window_focused`]).

use std::rc::Rc;
use std::time::Duration;

use futures::StreamExt as _;
use futures::channel::mpsc;
use futures::stream::BoxStream;
use gitto_git::features::watch::schema::GitDirChange;
use gitto_git::input::RepositoryId;
use gitto_git::{ApiError, GitApi};
use gpui_kit::{AppContext as _, Context, Entity, Task, WeakEntity};

use super::repository::Repository;

/// How long the working tree stays watched after the window loses focus. Starting to watch walks
/// the whole tree, so switching to an editor and back shouldn't start over every time.
pub const UNWATCH_AFTER: Duration = Duration::from_secs(5 * 60);

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
/// `super::repository`). The git directory (commits, checkouts, staging) is watched all the time.
/// The working tree is watched while the window has focus and for a while after; changes to it
/// while the window doesn't have focus are loaded once it gets focus back, and not at all if there
/// weren't any. A watch that stopped (e.g. Linux ran out of file watches) is started again, with a
/// reload, when the window gets focus.
pub struct RepositoryWatcher {
    id: RepositoryId,
    /// What it loads again: once the repository's removed from Gitto, nothing.
    repository: WeakEntity<Repository>,
    streams: Rc<dyn WatchStreams>,
    focused: bool,
    git_dir: Watch,
    working_tree: Watch,
    /// Whether the working tree changed while the window didn't have focus.
    changed: bool,
    unwatch_timer: Option<Task<()>>,
}

impl RepositoryWatcher {
    /// Starts watching `repository`, which just came on show, with the window focused or not.
    pub fn new(
        repository: &Entity<Repository>,
        streams: Rc<dyn WatchStreams>,
        focused: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut watcher = RepositoryWatcher {
            id: repository.read(cx).id().clone(),
            repository: repository.downgrade(),
            streams,
            focused,
            git_dir: Watch::new("the git directory"),
            working_tree: Watch::new("the working tree"),
            changed: false,
            unwatch_timer: None,
        };
        // Nothing watched the repository until now, so what was loaded from it before (when it was
        // last on show) may be out of date. What's loading isn't loaded twice: it's loading since
        // it came on show (see `Repository::reload_when_shown`).
        repository.update(cx, |repository, cx| repository.reload_when_shown(cx));
        // The view drops it as the repository goes off show.
        cx.on_release(|watcher: &mut Self, cx| {
            let _ = watcher
                .repository
                .update(cx, |repository, _| repository.went_off_show());
        })
        .detach();
        watcher.watch_git_dir(cx);
        watcher.focus_changed(None, cx);
        watcher
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
        let _ = self.repository.update(cx, |repository, cx| {
            drop(repository.reload_all(cx));
        });
    }

    fn reload_uncommitted(&self, cx: &mut Context<Self>) {
        let _ = self.repository.update(cx, |repository, cx| {
            drop(repository.reload_uncommitted(cx));
        });
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
mod tests;
