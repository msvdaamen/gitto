//! A line of tasks per key, run one at a time: the repositories' writes and fetches go through
//! one each. Ported from the write queue in `packages/git/src/core/runner.ts`.

use std::collections::{BTreeSet, HashMap};
use std::fmt::Debug;
use std::future::Future;
use std::hash::Hash;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, ready};
use std::time::{Duration, Instant};

use event_listener::Event;
use futures_lite::FutureExt;

use super::runner::lock;
use super::trace::TARGET;

/// How long a task waits for its turn before that's logged as a warning: much longer than any
/// write takes, so it's likely one that never comes (see `WriteQueue`).
const WAITED_LONG: Duration = Duration::from_secs(10);

/// Runs tasks for a key (a repository's path, say) one at a time: each waits until the ones before
/// it are done, whether they succeeded or not. A task that's dropped before its turn gives its
/// place up. There are two ways in:
///
/// - [`run`](Self::run) takes the task's place in line when it's called, and runs the task on
///   smol's global executor: tasks run in the order they were asked for, whichever threads poll
///   them, and none waits on one before it that isn't being polled. Once its turn has come, the
///   task runs to its end, even if its future is dropped. The repositories' writes go through it
///   (see `Repo::write`), which mustn't stop partway.
/// - [`turn`](Self::turn) takes a place when it's first polled, and holds it until it's dropped,
///   for a task the caller runs itself, which stops if it's dropped: to run such tasks in a given
///   order, poll them in it. The repositories' fetches go through it (see `Repo::fetch`).
///
/// A task that waits on a later one with the same key never gets its turn, nor does any after it:
/// a write inside `Repo::exclusive`, say. One that's waited over `WAITED_LONG` is logged as a
/// warning, naming the key.
///
/// Reads don't go through one: with GIT_OPTIONAL_LOCKS=0 they take no locks, so they can run
/// alongside anything.
pub(crate) struct WriteQueue<K = PathBuf> {
    /// Each key's line, while it has a task in it (see `Turn::drop`).
    lines: Arc<Mutex<HashMap<K, Arc<Line>>>>,
    waited_long: Duration,
}

impl<K> Default for WriteQueue<K> {
    fn default() -> Self {
        WriteQueue {
            lines: Arc::default(),
            waited_long: WAITED_LONG,
        }
    }
}

impl<K: Eq + Hash + Clone + Debug + Send + Sync + 'static> WriteQueue<K> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Runs `task` once the tasks before it with `key` are done; resolves to its output. Its place
    /// in line is taken now, and `task` runs whether or not the future is polled meanwhile, to its
    /// end once it's started; dropped before its turn, it doesn't run at all (see `WriteQueue`).
    pub fn run<T, F>(&self, key: K, task: F) -> Queued<T>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let place = Turn::take(Arc::clone(&self.lines), key);
        let waited_long = self.waited_long;
        let stage = Arc::new(AtomicU8::new(WAITING));
        let started = Arc::clone(&stage);
        let task = smol::spawn(async move {
            place.wait(waited_long).await;
            // Unless its future was dropped meanwhile; once started, it runs to its end.
            if started
                .compare_exchange(WAITING, STARTED, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            {
                return None;
            }
            let output = task.await;
            drop(place);
            Some(output)
        });
        Queued {
            task: Some(task),
            stage,
        }
    }

    /// A place in line for `key`, taken when it's first polled: resolves to a [`Turn`] once the
    /// ones before it are done, which holds the next ones back until it's dropped.
    pub fn turn(&self, key: K) -> impl Future<Output = Turn<K>> + Send + 'static + use<K> {
        let lines = Arc::clone(&self.lines);
        let waited_long = self.waited_long;
        async move {
            let place = Turn::take(lines, key);
            place.wait(waited_long).await;
            place
        }
    }

    /// The keys that have a line, for tests: one with no task left in it is forgotten.
    #[cfg(test)]
    fn keys(&self) -> Vec<K> {
        lock(&self.lines).keys().cloned().collect()
    }
}

/// Where a task from `WriteQueue::run` is (its `Queued::stage`): waiting for its turn, and then
/// either started or given up, by its future being dropped first.
const WAITING: u8 = 0;
/// See `WAITING`.
const STARTED: u8 = 1;
/// See `WAITING`.
const GIVEN_UP: u8 = 2;

/// A task in a `WriteQueue`'s line, from [`WriteQueue::run`]: resolves to its output.
pub(crate) struct Queued<T> {
    /// `None` once it's resolved.
    task: Option<smol::Task<Option<T>>>,
    stage: Arc<AtomicU8>,
}

impl<T> Future for Queued<T> {
    type Output = T;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<T> {
        let task = self.task.as_mut().expect("a task polled after it resolved");
        let output = ready!(Pin::new(task).poll(cx));
        self.task = None;
        // Only dropping this gives the task's place up.
        Poll::Ready(output.unwrap_or_else(|| unreachable!("a task given up while it's awaited")))
    }
}

impl<T> Drop for Queued<T> {
    fn drop(&mut self) {
        let Some(task) = self.task.take() else {
            return;
        };
        let given_up = self
            .stage
            .compare_exchange(WAITING, GIVEN_UP, Ordering::AcqRel, Ordering::Acquire)
            .is_ok();
        if given_up {
            // Cancelled, which drops its place in line.
            drop(task);
        } else {
            task.detach();
        }
    }
}

/// One key's line of tasks (see `WriteQueue`).
#[derive(Default)]
struct Line {
    state: Mutex<LineState>,
    event: Event,
}

#[derive(Default)]
struct LineState {
    /// The number the next place in line gets.
    next: u64,
    /// The number of the place whose turn it is.
    serving: u64,
    /// Places that were given up before their turn, skipped when it comes.
    given_up: BTreeSet<u64>,
}

/// A place in a `WriteQueue`'s line, whose turn it is once it's been waited for: the next task's
/// turn comes once it's dropped.
pub(crate) struct Turn<K: Eq + Hash = PathBuf> {
    lines: Arc<Mutex<HashMap<K, Arc<Line>>>>,
    key: K,
    line: Arc<Line>,
    number: u64,
}

impl<K: Eq + Hash + Clone + Debug> Turn<K> {
    /// The next place in `key`'s line, which is made if it has none. Under the lock of `lines`,
    /// as `drop` forgets a line under it, so no place is taken in a line that's been forgotten.
    fn take(lines: Arc<Mutex<HashMap<K, Arc<Line>>>>, key: K) -> Self {
        let (line, number) = {
            let mut map = lock(&lines);
            let line = Arc::clone(map.entry(key.clone()).or_default());
            let mut state = lock(&line.state);
            let number = state.next;
            state.next += 1;
            drop(state);
            (line, number)
        };
        Turn {
            lines,
            key,
            line,
            number,
        }
    }

    fn is_up(&self) -> bool {
        lock(&self.line.state).serving == self.number
    }

    /// Waits until it's this place's turn; warns once if that takes over `waited_long`.
    async fn wait(&self, waited_long: Duration) {
        let started = Instant::now();
        let mut warned = false;
        loop {
            if self.is_up() {
                return;
            }
            let listener = self.line.event.listen();
            if self.is_up() {
                return;
            }
            if warned {
                listener.await;
                continue;
            }
            let timed_out = async {
                listener.await;
                false
            }
            .or(async {
                async_io::Timer::at(started + waited_long).await;
                true
            })
            .await;
            if timed_out {
                warned = true;
                log::warn!(
                    target: TARGET,
                    "a write to {:?} has waited {}s for its turn: is one before it never polled, \
                     or waiting on this one, as a write inside `Repo::exclusive` does?",
                    self.key,
                    waited_long.as_secs_f32(),
                );
            }
        }
    }
}

impl<K: Eq + Hash> Drop for Turn<K> {
    fn drop(&mut self) {
        // The line's lock under that of the lines, as in `take`.
        let mut lines = lock(&self.lines);
        let mut guard = lock(&self.line.state);
        let state = &mut *guard;
        let notify = state.serving == self.number;
        if notify {
            state.serving += 1;
            while state.given_up.remove(&state.serving) {
                state.serving += 1;
            }
        } else {
            state.given_up.insert(self.number);
        }
        // Nobody's left in it, waiting or with their turn: forgotten, so a queue keyed by every
        // file ever saved, say, doesn't keep a line for each.
        if state.serving == state.next {
            lines.remove(&self.key);
        }
        drop(guard);
        drop(lines);
        if notify {
            self.line.event.notify(usize::MAX);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use futures_lite::future::poll_once;

    use super::*;

    #[test]
    fn runs_tasks_in_the_order_theyre_asked_for_however_theyre_polled() {
        smol::block_on(async {
            let queue = WriteQueue::<&str>::new();
            let order = Arc::new(Mutex::new(Vec::new()));
            let push = |number| {
                let order = Arc::clone(&order);
                async move { lock(&order).push(number) }
            };
            let first = queue.run("repo", {
                let push = push(1);
                async move {
                    async_io::Timer::after(Duration::from_millis(50)).await;
                    push.await;
                }
            });
            let second = queue.run("repo", push(2));
            let other = queue.run("other", push(3));
            // Polled in the opposite order to the one they were asked for in.
            futures::join!(other, second, first);
            assert_eq!(*lock(&order), [3, 1, 2]);
        });
    }

    #[test]
    fn runs_a_task_whose_future_isnt_polled_so_none_after_it_waits_for_that() {
        smol::block_on(async {
            let queue = WriteQueue::<&str>::new();
            let ran = Arc::new(Mutex::new(Vec::new()));
            let first = queue.run("repo", {
                let ran = Arc::clone(&ran);
                async move { lock(&ran).push(1) }
            });
            let second = queue.run("repo", async { 2 });
            // Awaited before the one asked for before it, which runs all the same.
            assert_eq!(second.await, 2);
            assert_eq!(*lock(&ran), [1]);
            first.await;
        });
    }

    #[test]
    fn runs_a_task_thats_dropped_during_its_turn_to_its_end() {
        smol::block_on(async {
            let queue = WriteQueue::<&str>::new();
            let (started, has_started) = async_channel::bounded(1);
            let (finished, has_finished) = async_channel::bounded(1);
            let task = queue.run("repo", async move {
                started.send(()).await.unwrap();
                async_io::Timer::after(Duration::from_millis(50)).await;
                finished.send(()).await.unwrap();
            });
            has_started.recv().await.unwrap();
            drop(task);
            has_finished.recv().await.unwrap();
            // And its turn ends with it.
            queue.run("repo", async {}).await;
        });
    }

    #[test]
    fn doesnt_run_a_task_dropped_before_its_turn() {
        smol::block_on(async {
            let queue = WriteQueue::<&str>::new();
            let first = queue.turn("repo").await;
            let ran = Arc::new(Mutex::new(false));
            let dropped = queue.run("repo", {
                let ran = Arc::clone(&ran);
                async move { *lock(&ran) = true }
            });
            let third = queue.run("repo", async { 3 });
            drop(dropped);
            drop(first);
            assert_eq!(third.await, 3);
            assert!(!*lock(&ran));
        });
    }

    #[test]
    fn turns_are_taken_in_the_order_theyre_first_polled() {
        smol::block_on(async {
            let queue = WriteQueue::<&str>::new();
            let first = queue.turn("repo");
            let second = queue.turn("repo");
            // Awaited before the one made before it, which waits meanwhile.
            drop(second.await);
            drop(first.await);
        });
    }

    #[test]
    fn skips_tasks_that_were_dropped() {
        smol::block_on(async {
            let queue = WriteQueue::<&str>::new();
            let first = queue.turn("repo").await;
            // Waiting behind the first, then dropped before its turn.
            let mut dropped = Box::pin(queue.turn("repo"));
            assert!(poll_once(&mut dropped).await.is_none());
            let mut third = Box::pin(queue.turn("repo"));
            assert!(poll_once(&mut third).await.is_none());
            drop(dropped);
            drop(first);
            third.await;
        });
    }

    #[test]
    fn forgets_a_line_once_nobody_is_in_it() {
        smol::block_on(async {
            let queue = WriteQueue::<&str>::new();
            let first = queue.turn("repo").await;
            let mut second = Box::pin(queue.turn("repo"));
            assert!(poll_once(&mut second).await.is_none());
            drop(first);
            assert_eq!(queue.keys(), ["repo"]);
            drop(second.await);
            assert!(queue.keys().is_empty());

            // Also when the last one gave its place up before its turn.
            let first = queue.turn("repo").await;
            let mut second = Box::pin(queue.turn("repo"));
            assert!(poll_once(&mut second).await.is_none());
            drop(second);
            assert_eq!(queue.keys(), ["repo"]);
            drop(first);
            assert!(queue.keys().is_empty());
            // And taken again as a new one.
            queue.run("repo", async {}).await;
            assert!(queue.keys().is_empty());
        });
    }

    #[test]
    fn warns_of_a_task_thats_waited_long_for_its_turn() {
        let logged = crate::test_support::logged_warnings();
        smol::block_on(async {
            let queue = WriteQueue::<&str> {
                waited_long: Duration::from_millis(50),
                ..WriteQueue::new()
            };
            let first = queue.turn("waited-long").await;
            let second = queue.turn("waited-long");
            let release = async {
                async_io::Timer::after(Duration::from_millis(300)).await;
                drop(first);
            };
            futures::join!(second, release);
        });
        let warnings = logged();
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].starts_with("a write to \"waited-long\" has waited 0.05s for its turn"),
            "{}",
            warnings[0]
        );
    }
}
