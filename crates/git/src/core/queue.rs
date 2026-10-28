//! A line of tasks per key, run one at a time: the repositories' writes and fetches go through
//! one each. Ported from the write queue in `packages/git/src/core/runner.ts`.

use std::collections::{BTreeSet, HashMap};
use std::fmt::Debug;
use std::future::Future;
use std::hash::Hash;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use event_listener::Event;
use futures_lite::FutureExt;

use super::runner::lock;
use super::trace::TARGET;

/// How long a task waits for its turn before that's logged as a warning: much longer than any
/// write takes, so it's likely one that never comes (see `WriteQueue`).
const WAITED_LONG: Duration = Duration::from_secs(10);

/// Runs tasks for a key (a repository's path, say) one at a time, in the order they're first
/// polled: each waits until the ones before it are done, whether they succeeded or not. Its place
/// in line is taken when its future is first polled, not when it's made, so futures can be made
/// ahead and awaited in any order without one waiting on another that isn't being polled; to run
/// tasks in a given order, poll them in it (start each before the next, say). A task that's
/// dropped, before its turn or during it, gives its place up.
///
/// A task that waits on a later one with the same key never gets its turn, nor does any after it:
/// a write inside `Repo::exclusive`, say. One that's waited over `WAITED_LONG` is logged as a
/// warning, naming the key.
///
/// The repositories' writes go through one (see `Repo::write`). Reads don't: with
/// GIT_OPTIONAL_LOCKS=0 they take no locks, so they can run alongside anything.
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
    /// in line is taken when it's first polled.
    pub fn run<T, F>(&self, key: K, task: F) -> impl Future<Output = T> + use<K, T, F>
    where
        F: Future<Output = T>,
    {
        let turn = self.turn(key);
        async move {
            let _turn = turn.await;
            task.await
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
    fn runs_tasks_in_the_order_theyre_first_polled() {
        smol::block_on(async {
            let queue = WriteQueue::<&str>::new();
            let order = Mutex::new(Vec::new());
            let first = queue.run("repo", async {
                async_io::Timer::after(Duration::from_millis(50)).await;
                lock(&order).push(1);
            });
            let second = queue.run("repo", async { lock(&order).push(2) });
            let other = queue.run("other", async { lock(&order).push(3) });
            // Polled in the opposite order to the one they were made in.
            futures::join!(other, second, first);
            assert_eq!(*lock(&order), [3, 2, 1]);

            let order = Mutex::new(Vec::new());
            let first = queue.run("repo", async {
                async_io::Timer::after(Duration::from_millis(50)).await;
                lock(&order).push(1);
            });
            let second = queue.run("repo", async { lock(&order).push(2) });
            futures::join!(first, second);
            assert_eq!(*lock(&order), [1, 2]);
        });
    }

    #[test]
    fn doesnt_hold_a_task_up_for_one_made_before_it_that_isnt_polled() {
        smol::block_on(async {
            let queue = WriteQueue::<&str>::new();
            let first = queue.run("repo", async { 1 });
            let second = queue.run("repo", async { 2 });
            // Awaited before the one made before it, which waits meanwhile.
            assert_eq!(second.await, 2);
            assert_eq!(first.await, 1);
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
        // Only this test's: the others log warnings too, as the api's failures.
        let warnings: Vec<String> = logged()
            .into_iter()
            .filter(|warning| warning.contains("\"waited-long\""))
            .collect();
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].starts_with("a write to \"waited-long\" has waited 0.05s for its turn"),
            "{}",
            warnings[0]
        );
    }
}
