//! Streams written as an async body that yields, as TS's async generators are: the body only runs
//! while the stream is polled for its next item, and stops (dropped) when the stream is.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use futures::Stream;
use futures::future::BoxFuture;

use crate::core::runner::lock;

/// What the body of a [`Generator`] yields its items with.
pub(super) struct Yielder<T> {
    slot: Arc<Mutex<Option<T>>>,
}

impl<T> Yielder<T> {
    /// Hands `item` to the stream's reader, and resumes once the next item is asked for.
    pub async fn put(&self, item: T) {
        *lock(&self.slot) = Some(item);
        Suspend(false).await;
    }
}

/// Pending once, so the generator returns the item in the slot; ready when polled again, for the
/// next item.
struct Suspend(bool);

impl Future for Suspend {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
        if self.0 {
            Poll::Ready(())
        } else {
            // Not woken: the generator returns the item, and is polled again for the next one.
            self.0 = true;
            Poll::Pending
        }
    }
}

/// A stream of what its body yields (see [`generate`]); it ends when the body does.
pub(super) struct Generator<T> {
    slot: Arc<Mutex<Option<T>>>,
    body: Option<BoxFuture<'static, ()>>,
}

/// The stream of the items `body` yields through the [`Yielder`] it's given.
pub(super) fn generate<T, F, Fut>(body: F) -> Generator<T>
where
    F: FnOnce(Yielder<T>) -> Fut,
    Fut: Future<Output = ()> + Send + 'static,
{
    let slot = Arc::new(Mutex::new(None));
    let yielder = Yielder {
        slot: Arc::clone(&slot),
    };
    Generator {
        slot,
        body: Some(Box::pin(body(yielder))),
    }
}

impl<T> Stream for Generator<T> {
    type Item = T;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
        let Some(body) = self.body.as_mut() else {
            return Poll::Ready(None);
        };
        match body.as_mut().poll(cx) {
            Poll::Ready(()) => {
                self.body = None;
                Poll::Ready(lock(&self.slot).take())
            }
            Poll::Pending => match lock(&self.slot).take() {
                Some(item) => Poll::Ready(Some(item)),
                None => Poll::Pending,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use futures::StreamExt;

    use super::*;

    #[test]
    fn yields_what_its_body_puts_until_it_ends() {
        smol::block_on(async {
            let counted = generate(|y| async move {
                for i in 0..3 {
                    async_io::Timer::after(std::time::Duration::from_millis(1)).await;
                    y.put(i).await;
                }
            });
            assert_eq!(counted.collect::<Vec<_>>().await, [0, 1, 2]);
        });
    }
}
