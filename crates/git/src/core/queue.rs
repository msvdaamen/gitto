//! A line of tasks per key, run one at a time: the repositories' writes and fetches go through
//! one each. Ported from the write queue in `packages/git/src/core/runner.ts`.

use std::collections::{BTreeSet, HashMap};
use std::future::Future;
use std::hash::Hash;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use event_listener::Event;

use super::runner::lock;

/// Runs tasks for a key (a repository's path, say) one at a time, in the order they're asked for:
/// each waits until the ones asked for before it are done, whether they succeeded or not. Its place
/// in line is taken when it's asked for (`run` or `turn` is called), not when it's first polled; a
/// task that's dropped, before its turn or during it, gives its place up.
///
/// The repositories' writes go through one (see `Repo::write`). Reads don't: with
/// GIT_OPTIONAL_LOCKS=0 they take no locks, so they can run alongside anything.
pub(crate) struct WriteQueue<K = PathBuf> {
    lines: Mutex<HashMap<K, Arc<Line>>>,
}

impl<K> Default for WriteQueue<K> {
    fn default() -> Self {
        WriteQueue {
            lines: Mutex::new(HashMap::new()),
        }
    }
}

impl<K: Eq + Hash> WriteQueue<K> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Runs `task` once the tasks asked for before it with `key` are done; resolves to its output.
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

    /// A place in line for `key`, taken now: resolves to a [`Turn`] once the ones before it are
    /// done, which holds the next ones back until it's dropped.
    pub fn turn(&self, key: K) -> impl Future<Output = Turn> + Send + 'static + use<K> {
        let line = Arc::clone(lock(&self.lines).entry(key).or_default());
        let number = {
            let mut state = lock(&line.state);
            let number = state.next;
            state.next += 1;
            number
        };
        let place = Turn { line, number };
        async move {
            loop {
                if place.is_up() {
                    return place;
                }
                let listener = place.line.event.listen();
                if place.is_up() {
                    return place;
                }
                listener.await;
            }
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
pub(crate) struct Turn {
    line: Arc<Line>,
    number: u64,
}

impl Turn {
    fn is_up(&self) -> bool {
        lock(&self.line.state).serving == self.number
    }
}

impl Drop for Turn {
    fn drop(&mut self) {
        let mut guard = lock(&self.line.state);
        let state = &mut *guard;
        if state.serving == self.number {
            state.serving += 1;
            while state.given_up.remove(&state.serving) {
                state.serving += 1;
            }
            drop(guard);
            self.line.event.notify(usize::MAX);
        } else {
            state.given_up.insert(self.number);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn write_queue_runs_tasks_in_the_order_asked_for() {
        smol::block_on(async {
            let queue = WriteQueue::<&str>::new();
            let order = Mutex::new(Vec::new());
            let first = queue.run("repo", async {
                async_io::Timer::after(Duration::from_millis(50)).await;
                lock(&order).push(1);
            });
            let second = queue.run("repo", async { lock(&order).push(2) });
            let other = queue.run("other", async { lock(&order).push(3) });
            // Polled in the opposite order to the one they were asked for.
            futures::join!(other, second, first);
            assert_eq!(*lock(&order), [3, 1, 2]);
        });
    }

    #[test]
    fn write_queue_skips_tasks_that_were_dropped() {
        smol::block_on(async {
            let queue = WriteQueue::<&str>::new();
            let first = queue.turn("repo");
            let dropped = queue.turn("repo");
            let third = queue.turn("repo");
            drop(dropped);
            let turn = first.await;
            drop(turn);
            third.await;
        });
    }
}
