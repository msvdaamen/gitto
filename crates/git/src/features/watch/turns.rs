//! Taking turns between subscribing and unsubscribing. Ported from `turns.ts`.

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use event_listener::Event;

use crate::core::runner::lock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Turn {
    Subscribe,
    Unsubscribe,
}

/// Runs tasks of one kind together, and the other kind once those are done. A subscribe goes ahead
/// of the unsubscribes that are waiting: a repository that's just been switched to isn't watched
/// until it's done, so it doesn't wait for one that's being left to finish walking its folders.
///
/// Cheap to clone; clones share their turns.
#[derive(Clone, Default)]
pub(super) struct Turns {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Default)]
struct Inner {
    running: Option<Turn>,
    count: usize,
    waiting: Vec<Arc<Ticket>>,
}

/// A task's place in line.
struct Ticket {
    kind: Turn,
    started: AtomicBool,
    start: Event,
}

impl Turns {
    /// Runs `task` once it's its turn, which is decided now: a task taken earlier, waiting for
    /// this one's kind to be done, isn't overtaken by a later one. Its turn ends when it does, or
    /// when the future is dropped, also if it panics.
    pub fn take<T, F, Fut>(&self, kind: Turn, task: F) -> impl Future<Output = T> + use<T, F, Fut>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = T>,
    {
        let ticket = Arc::new(Ticket {
            kind,
            started: AtomicBool::new(false),
            start: Event::new(),
        });
        {
            let mut inner = lock(&self.inner);
            // Along with what's running, if that's of its kind and no subscribe is waiting for it.
            let joins = inner.running == Some(kind)
                && (kind == Turn::Subscribe
                    || !inner
                        .waiting
                        .iter()
                        .any(|other| other.kind == Turn::Subscribe));
            if inner.count == 0 || joins {
                inner.start(&ticket);
            } else {
                inner.waiting.push(Arc::clone(&ticket));
            }
        }
        let place = Place {
            turns: Arc::clone(&self.inner),
            ticket,
        };
        async move {
            place.started().await;
            let output = task().await;
            drop(place);
            output
        }
    }
}

impl Inner {
    fn start(&mut self, ticket: &Ticket) {
        self.running = Some(ticket.kind);
        self.count += 1;
        ticket.started.store(true, Ordering::Release);
        ticket.start.notify(usize::MAX);
    }

    /// Starts what's waiting of one kind: the subscribes, if there are any.
    fn next(&mut self) {
        self.running = None;
        let kind = if self
            .waiting
            .iter()
            .any(|other| other.kind == Turn::Subscribe)
        {
            Turn::Subscribe
        } else {
            Turn::Unsubscribe
        };
        let (starting, waiting) = std::mem::take(&mut self.waiting)
            .into_iter()
            .partition::<Vec<_>, _>(|other| other.kind == kind);
        self.waiting = waiting;
        for other in starting {
            self.start(&other);
        }
    }
}

/// A task's place in line, given up when it's dropped: the turn ends if it had started, and the
/// next kind starts if it was the last one of its kind running.
struct Place {
    turns: Arc<Mutex<Inner>>,
    ticket: Arc<Ticket>,
}

impl Place {
    async fn started(&self) {
        loop {
            if self.ticket.started.load(Ordering::Acquire) {
                return;
            }
            let listener = self.ticket.start.listen();
            if self.ticket.started.load(Ordering::Acquire) {
                return;
            }
            listener.await;
        }
    }
}

impl Drop for Place {
    fn drop(&mut self) {
        let mut inner = lock(&self.turns);
        if self.ticket.started.load(Ordering::Acquire) {
            inner.count -= 1;
            if inner.count == 0 {
                inner.next();
            }
        } else {
            inner
                .waiting
                .retain(|other| !Arc::ptr_eq(other, &self.ticket));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::panic::AssertUnwindSafe;
    use std::pin::pin;

    use futures::FutureExt;
    use futures_lite::future::poll_once;

    use super::*;

    /// A task that says when it starts, and ends when `end` is sent.
    fn task(
        order: &Arc<Mutex<Vec<&'static str>>>,
        name: &'static str,
    ) -> (
        async_channel::Sender<()>,
        impl FnOnce() -> futures::future::BoxFuture<'static, ()> + use<>,
    ) {
        let (end, ended) = async_channel::bounded::<()>(1);
        let order = Arc::clone(order);
        let run = move || {
            lock(&order).push(name);
            async move {
                let _ = ended.recv().await;
            }
            .boxed()
        };
        (end, run)
    }

    #[test]
    fn start_alongside_each_other_and_ahead_of_an_unsubscribe_thats_waiting_for_one() {
        smol::block_on(async {
            let turns = Turns::default();
            let order = Arc::new(Mutex::new(Vec::new()));
            let (walking_end, walking) = task(&order, "walking");
            let (leaving_end, leaving) = task(&order, "leaving");
            let (shown_end, shown) = task(&order, "shown");

            // The repository that's left is still starting to watch its folders when it's
            // unwatched.
            let mut first = pin!(turns.take(Turn::Subscribe, walking));
            let mut unsubscribed = pin!(turns.take(Turn::Unsubscribe, leaving));
            // The one that's shown doesn't wait for either.
            let mut second = pin!(turns.take(Turn::Subscribe, shown));
            assert!(poll_once(&mut first).await.is_none());
            assert!(poll_once(&mut unsubscribed).await.is_none());
            assert!(poll_once(&mut second).await.is_none());
            assert_eq!(*lock(&order), ["walking", "shown"]);

            shown_end.send(()).await.unwrap();
            second.await;
            assert!(poll_once(&mut unsubscribed).await.is_none());
            assert_eq!(*lock(&order), ["walking", "shown"]);
            walking_end.send(()).await.unwrap();
            first.await;
            assert!(poll_once(&mut unsubscribed).await.is_none());
            assert_eq!(*lock(&order), ["walking", "shown", "leaving"]);
            leaving_end.send(()).await.unwrap();
            unsubscribed.await;
        });
    }

    #[test]
    fn carry_on_after_one_fails() {
        smol::block_on(async {
            let turns = Turns::default();
            let failed = turns
                .take(Turn::Subscribe, || async { Err::<(), _>("no") })
                .await;
            assert!(failed.is_err());
            // Panics rather than fails.
            let panicked = AssertUnwindSafe(turns.take(Turn::Unsubscribe, || async {
                panic!("no");
            }))
            .catch_unwind()
            .await;
            assert!(panicked.is_err());
            assert_eq!(
                turns.take(Turn::Subscribe, || async { "watching" }).await,
                "watching"
            );
        });
    }

    #[test]
    fn give_up_their_place_when_dropped_before_their_turn() {
        smol::block_on(async {
            let turns = Turns::default();
            let order = Arc::new(Mutex::new(Vec::new()));
            let (walking_end, walking) = task(&order, "walking");
            let mut first = pin!(turns.take(Turn::Subscribe, walking));
            assert!(poll_once(&mut first).await.is_none());
            drop(turns.take(Turn::Unsubscribe, || async {}));
            walking_end.send(()).await.unwrap();
            first.await;
            assert_eq!(turns.take(Turn::Unsubscribe, || async { 1 }).await, 1);
        });
    }
}
