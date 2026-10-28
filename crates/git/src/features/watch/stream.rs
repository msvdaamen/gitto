//! The streams the watch commands return: TS's async generators, as a [`Watched`] state that's
//! asked for its next item while the stream is polled for one, and dropped with the stream.

use std::future::Future;

use futures::Stream;
use futures::stream;

use crate::core::errors::Result;

/// What a stream watches, once it's set up.
pub(super) trait Watched: Send + Sized + 'static {
    type Item: Send + 'static;

    /// The next item; `None` once there are no more. An error ends the stream.
    fn next(&mut self) -> impl Future<Output = Option<Result<Self::Item>>> + Send;

    /// Stops watching, once the stream ended. Dropping it stops it too, in the background.
    fn stop(self) -> impl Future<Output = ()> + Send;
}

/// What `start` sets up then yields, until it ends or fails. Only set up once it's polled, and
/// only asked for an item when one is: a change is only acted on (e.g. by reading the status) once
/// the one before it was taken. What's watched is stopped before the error that ended it is
/// yielded, and in the background if the stream is dropped first.
pub(super) fn watch_stream<W: Watched>(
    start: impl Future<Output = Result<W>> + Send + 'static,
) -> impl Stream<Item = Result<W::Item>> + Send + Unpin + 'static {
    enum State<F, W> {
        Starting(F),
        Watching(W),
        Ended,
    }
    Box::pin(stream::unfold(State::Starting(start), |state| async move {
        let mut watched = match state {
            State::Starting(start) => match start.await {
                Ok(watched) => watched,
                Err(error) => return Some((Err(error), State::Ended)),
            },
            State::Watching(watched) => watched,
            State::Ended => return None,
        };
        match watched.next().await {
            Some(Ok(item)) => Some((Ok(item), State::Watching(watched))),
            ended => {
                watched.stop().await;
                ended.map(|error| (error, State::Ended))
            }
        }
    }))
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use futures::StreamExt;

    use super::*;
    use crate::core::errors::Error;
    use crate::core::runner::lock;

    /// Counts up to `until`, then fails if `fails`; says when it's stopped.
    struct Counting {
        next: u32,
        until: u32,
        fails: bool,
        log: Arc<Mutex<Vec<String>>>,
    }

    impl Watched for Counting {
        type Item = u32;

        async fn next(&mut self) -> Option<Result<u32>> {
            if self.next == self.until {
                return self.fails.then(|| Err(Error::Bug("failed".into())));
            }
            self.next += 1;
            lock(&self.log).push(format!("next {}", self.next));
            Some(Ok(self.next))
        }

        async fn stop(self) {
            lock(&self.log).push("stopped".into());
        }
    }

    fn counting(until: u32, fails: bool) -> (Counting, Arc<Mutex<Vec<String>>>) {
        let log = Arc::new(Mutex::new(Vec::new()));
        let counting = Counting {
            next: 0,
            until,
            fails,
            log: Arc::clone(&log),
        };
        (counting, log)
    }

    fn items(results: Vec<Result<u32>>) -> Vec<Result<u32, String>> {
        results
            .into_iter()
            .map(|result| result.map_err(|error| error.to_string()))
            .collect()
    }

    #[test]
    fn yields_until_there_are_no_more_then_stops() {
        smol::block_on(async {
            let (counted, log) = counting(2, false);
            let stream = watch_stream(async { Ok(counted) });
            assert_eq!(items(stream.collect().await), [Ok(1), Ok(2)]);
            assert_eq!(*lock(&log), ["next 1", "next 2", "stopped"]);
        });
    }

    #[test]
    fn stops_before_yielding_the_error_that_ended_it() {
        smol::block_on(async {
            let (counted, log) = counting(1, true);
            let mut stream = watch_stream(async { Ok(counted) });
            assert_eq!(stream.next().await.unwrap().unwrap(), 1);
            assert!(stream.next().await.unwrap().is_err());
            assert_eq!(*lock(&log), ["next 1", "stopped"]);
            assert!(stream.next().await.is_none());
        });
    }

    #[test]
    fn yields_the_error_it_couldnt_start_with_and_ends() {
        smol::block_on(async {
            let stream = watch_stream::<Counting>(async { Err(Error::Bug("no".into())) });
            assert_eq!(items(stream.collect().await), [Err("no".to_owned())]);
        });
    }

    #[test]
    fn only_asks_for_an_item_when_one_is_asked_for() {
        smol::block_on(async {
            let (counted, log) = counting(5, false);
            let mut stream = watch_stream(async { Ok(counted) });
            assert!(lock(&log).is_empty());
            stream.next().await;
            stream.next().await;
            assert_eq!(*lock(&log), ["next 1", "next 2"]);
        });
    }
}
