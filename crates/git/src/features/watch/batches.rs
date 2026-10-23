//! Debouncing what the watchers report. Ported from `batches.ts`.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use async_io::Timer;
use event_listener::Event;

use crate::core::errors::{Error, Result};
use crate::core::runner::lock;

const DEBOUNCE: Duration = Duration::from_millis(300);
/// A batch is handed over this long after its first change at the latest. Changes that keep coming
/// without a pause, like a build writing its output, would otherwise hold it back until they stop.
const MAX_WAIT: Duration = Duration::from_millis(2000);

/// Collects what the watchers report and hands it over in batches: once it's quiet for a bit, or
/// once the batch has waited long enough. Shared between the watchers' threads, which add to it,
/// and the one reading it with `next`.
pub(super) struct Batches<T> {
    state: Mutex<State<T>>,
    /// Told whenever `state` changes.
    changed: Event,
}

/// The batches, on a clock of their own: TS's timers, as the times they'd fire at.
struct State<T> {
    /// The batch being collected.
    pending: Vec<T>,
    /// Batches whose time came before they were asked for, in order.
    ready: VecDeque<Vec<T>>,
    /// When the pending batch is handed over at the latest.
    deadline: Option<Instant>,
    /// When the pending batch is handed over, unless more is added before.
    due: Option<Instant>,
    /// What ends the batches, e.g. when there are no file watches left (ENOSPC on Linux).
    error: Option<Error>,
    closed: bool,
}

impl<T> State<T> {
    fn new() -> Self {
        State {
            pending: Vec::new(),
            ready: VecDeque::new(),
            deadline: None,
            due: None,
            error: None,
            closed: false,
        }
    }

    /// Adds `values` at `now`, putting the batch off once for all of them.
    fn add_all(&mut self, values: Vec<T>, now: Instant) {
        if self.closed || values.is_empty() {
            return;
        }
        // Handed over already, as far as the clock goes: before these were added.
        if self.due.is_some_and(|due| now >= due) {
            self.hand_over();
        }
        self.pending.extend(values);
        let deadline = *self.deadline.get_or_insert(now + MAX_WAIT);
        self.due = Some((now + DEBOUNCE).min(deadline));
    }

    fn hand_over(&mut self) {
        self.ready.push_back(std::mem::take(&mut self.pending));
        self.deadline = None;
        self.due = None;
    }

    /// The next batch that's been handed over by `now`.
    fn take(&mut self, now: Instant) -> Option<Vec<T>> {
        if self.due.is_some_and(|due| now >= due) {
            self.hand_over();
        }
        self.ready.pop_front()
    }
}

impl<T> Batches<T> {
    pub fn new() -> Self {
        Batches {
            state: Mutex::new(State::new()),
            changed: Event::new(),
        }
    }

    pub fn add(&self, value: T) {
        self.add_all(vec![value]);
    }

    /// Adds `values` at once, as a watcher reports its changes: the batch is put off once for all
    /// of them, rather than once per change, of which a build can report thousands at a time.
    pub fn add_all(&self, values: Vec<T>) {
        if values.is_empty() {
            return;
        }
        lock(&self.state).add_all(values, Instant::now());
        self.changed.notify(usize::MAX);
    }

    /// Ends the batches with `error`, e.g. when there are no file watches left (ENOSPC on Linux).
    pub fn fail(&self, error: Error) {
        {
            let mut state = lock(&self.state);
            if state.closed {
                return;
            }
            state.error.get_or_insert(error);
        }
        self.changed.notify(usize::MAX);
    }

    /// The next batch, once it's handed over; `None` once they're closed. An error ends them: the
    /// batches handed over before it come first.
    pub async fn next(&self) -> Option<Result<Vec<T>>> {
        loop {
            let (listener, due) = {
                let mut state = lock(&self.state);
                if let Some(batch) = state.ready.pop_front() {
                    return Some(Ok(batch));
                }
                if let Some(error) = state.error.take() {
                    state.closed = true;
                    return Some(Err(error));
                }
                if state.closed {
                    return None;
                }
                if let Some(batch) = state.take(Instant::now()) {
                    return Some(Ok(batch));
                }
                (self.changed.listen(), state.due)
            };
            match due {
                Some(due) => {
                    futures_lite::future::or(listener, async {
                        Timer::at(due).await;
                    })
                    .await
                }
                None => listener.await,
            }
        }
    }

    /// Ends the batches: `next` returns `None` from now on, and what's added is dropped.
    pub fn close(&self) {
        lock(&self.state).closed = true;
        self.changed.notify(usize::MAX);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The batches on a clock of the test's own, as TS's tests use fake timers: `advance` moves it
    /// on, collecting what's handed over meanwhile.
    struct Clock {
        state: State<u32>,
        now: Instant,
        handed: Vec<Vec<u32>>,
    }

    impl Clock {
        fn new() -> Self {
            Clock {
                state: State::new(),
                now: Instant::now(),
                handed: Vec::new(),
            }
        }

        fn add(&mut self, value: u32) {
            self.add_all(vec![value]);
        }

        fn add_all(&mut self, values: Vec<u32>) {
            self.state.add_all(values, self.now);
        }

        /// Moves the clock on by `ms`, a millisecond at a time, as timers fire in order.
        fn advance(&mut self, ms: u64) {
            for _ in 0..ms {
                self.now += Duration::from_millis(1);
                while let Some(batch) = self.state.take(self.now) {
                    self.handed.push(batch);
                }
            }
        }

        fn flat(&self) -> Vec<u32> {
            self.handed.iter().flatten().copied().collect()
        }
    }

    #[test]
    fn hands_changes_over_together_once_its_quiet_for_a_bit() {
        let mut batches = Clock::new();

        batches.add(1);
        batches.advance(200);
        batches.add(2);
        batches.advance(299);
        assert!(batches.handed.is_empty());
        batches.advance(1);
        assert_eq!(batches.handed, [vec![1, 2]]);
    }

    #[test]
    fn takes_several_changes_at_once_as_one_batch() {
        let mut batches = Clock::new();

        batches.add_all(vec![1, 2]);
        batches.add_all(vec![]);
        batches.advance(300);
        assert_eq!(batches.handed, [vec![1, 2]]);
    }

    #[test]
    fn doesnt_hold_changes_back_for_as_long_as_they_keep_coming() {
        let mut batches = Clock::new();

        // A change every 100ms, for five seconds: never quiet for long enough.
        for i in 0..50 {
            batches.add(i);
            batches.advance(100);
        }
        assert_eq!(batches.handed.len(), 2);
        assert_eq!(batches.flat(), (0..40).collect::<Vec<_>>());

        batches.advance(300);
        assert_eq!(batches.flat(), (0..50).collect::<Vec<_>>());
    }

    #[test]
    fn hands_batches_over_in_real_time_and_ends_when_closed() {
        smol::block_on(async {
            let batches = Batches::new();
            let start = Instant::now();
            batches.add_all(vec![1, 2]);
            assert_eq!(batches.next().await.unwrap().unwrap(), [1, 2]);
            assert!(start.elapsed() >= DEBOUNCE);
            batches.add(3);
            batches.close();
            assert!(batches.next().await.is_none());
        });
    }

    #[test]
    fn ends_with_an_error_after_what_was_handed_over() {
        smol::block_on(async {
            let batches = Batches::new();
            batches.add(1);
            Timer::after(DEBOUNCE + Duration::from_millis(50)).await;
            batches.add(2);
            batches.fail(Error::Bug("no watches left".into()));
            assert_eq!(batches.next().await.unwrap().unwrap(), [1]);
            assert!(batches.next().await.unwrap().is_err());
            assert!(batches.next().await.is_none());
        });
    }
}
