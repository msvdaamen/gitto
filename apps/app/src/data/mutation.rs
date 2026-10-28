//! A change a view makes, with its state: TanStack Query's `useMutation`, for the changes that
//! aren't a repository's operations (see `super::operation`), like committing or staging.
//!
//! A [`Mutation`] is an entity a view makes in its constructor and observes
//! (`cx.observe(&mutation, |_, _, cx| cx.notify())`): whether it's running, why it last failed,
//! and the input it was last run with (TanStack's `variables`), until it's [`reset`](Mutation::reset).
//! Unlike an operation, its state is the view's own, and it runs again even while it's running,
//! as TanStack's does: what it shows is of the last run.

use std::rc::Rc;

use futures::channel::oneshot;
use gpui_kit::{App, AppContext as _, Context, Entity, Task};

use super::client::QueryError;

/// Runs a mutation with its input, on the main thread; its task runs the slow part on the
/// background executor, and settles once what it reloads is reloaded, if it waits for that.
pub type RunMutation<I, R> = Rc<dyn Fn(I, &mut App) -> Task<Result<R, QueryError>>>;

/// A change, and the state of its last run (see the module's doc).
pub struct Mutation<I, R = ()> {
    run: RunMutation<I, R>,
    pending: bool,
    error: Option<QueryError>,
    input: Option<I>,
    /// Counts the runs and resets, so a run that settles after another started, or after a
    /// reset, leaves the state alone.
    generation: u64,
}

impl<I: Clone + 'static, R: 'static> Mutation<I, R> {
    /// The mutation `run` does.
    pub fn new(
        run: impl Fn(I, &mut App) -> Task<Result<R, QueryError>> + 'static,
        cx: &mut App,
    ) -> Entity<Self> {
        cx.new(|_| Mutation {
            run: Rc::new(run),
            pending: false,
            error: None,
            input: None,
            generation: 0,
        })
    }

    /// Runs it with `input`, for what it shows (its state, and the error it fails with): what it
    /// resolves to isn't waited for. Most changes are run so; see [`mutate`](Self::mutate) to act
    /// on the result too.
    pub fn run(&mut self, input: I, cx: &mut Context<Self>) {
        drop(self.mutate(input, cx));
    }

    /// Runs it with `input` (TanStack's `mutate`). It runs to the end whatever happens to the
    /// view that ran it; the receiver is told what it resolved to, for that view to act on if it's
    /// still there (TS's `onSuccess` given to `mutate`). It's only cancelled if Gitto quits
    /// first.
    pub fn mutate(
        &mut self,
        input: I,
        cx: &mut Context<Self>,
    ) -> oneshot::Receiver<Result<R, QueryError>> {
        self.generation += 1;
        let generation = self.generation;
        self.pending = true;
        self.error = None;
        self.input = Some(input.clone());
        cx.notify();
        let work = (self.run)(input, cx);
        let (done, result) = oneshot::channel();
        cx.spawn(async move |this, cx| {
            let outcome = work.await;
            let _ = this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                this.pending = false;
                this.error = outcome.as_ref().err().cloned();
                cx.notify();
            });
            let _ = done.send(outcome);
        })
        .detach();
        result
    }

    /// Whether its last run is under way (TanStack's `isPending`).
    pub fn is_pending(&self) -> bool {
        self.pending
    }

    /// Why its last run failed, if it did.
    pub fn error(&self) -> Option<&QueryError> {
        self.error.as_ref()
    }

    /// The input of its last run (TanStack's `variables`), e.g. to tell whether it's of the
    /// repository on show.
    pub fn input(&self) -> Option<&I> {
        self.input.as_ref()
    }

    /// Forgets its last run: not running, nor failed (TanStack's `reset`). A run under way goes
    /// on, but no longer shows.
    pub fn reset(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        if self.pending || self.error.is_some() || self.input.is_some() {
            self.pending = false;
            self.error = None;
            self.input = None;
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use futures::FutureExt as _;
    use gpui_kit::TestAppContext;

    use super::*;

    /// What finishes each run, in turn.
    type Finishes = Rc<RefCell<Vec<oneshot::Sender<Result<u32, QueryError>>>>>;

    /// A mutation whose runs finish when the test says.
    fn mutation(cx: &mut TestAppContext) -> (Entity<Mutation<&'static str, u32>>, Finishes) {
        let finishes = Rc::new(RefCell::new(Vec::new()));
        let mutation = cx.update(|cx| {
            let finishes = Rc::clone(&finishes);
            Mutation::new(
                move |_, cx: &mut App| {
                    let (finish, finished) = oneshot::channel();
                    finishes.borrow_mut().push(finish);
                    cx.background_spawn(async move { finished.await.unwrap() })
                },
                cx,
            )
        });
        (mutation, finishes)
    }

    #[gpui_kit::test]
    fn says_whether_its_last_run_is_under_way_and_why_it_failed(cx: &mut TestAppContext) {
        let (mutation, finishes) = mutation(cx);
        let result = mutation.update(cx, |mutation, cx| mutation.mutate("a", cx));
        assert!(mutation.read_with(cx, |mutation, _| mutation.is_pending()));
        assert_eq!(
            mutation.read_with(cx, |mutation, _| mutation.input().copied()),
            Some("a")
        );

        let finish = finishes.borrow_mut().remove(0);
        finish
            .send(Err(QueryError::msg("Nothing to commit.")))
            .unwrap();
        cx.run_until_parked();
        mutation.read_with(cx, |mutation, _| {
            assert!(!mutation.is_pending());
            assert_eq!(mutation.error().unwrap().message(), "Nothing to commit.");
        });
        assert!(result.now_or_never().unwrap().unwrap().is_err());

        mutation.update(cx, |mutation, cx| mutation.reset(cx));
        mutation.read_with(cx, |mutation, _| {
            assert!(mutation.error().is_none());
            assert!(mutation.input().is_none());
        });
    }

    #[gpui_kit::test]
    fn shows_its_last_run_not_one_from_before(cx: &mut TestAppContext) {
        let (mutation, finishes) = mutation(cx);
        mutation.update(cx, |mutation, cx| mutation.run("a", cx));
        let second = mutation.update(cx, |mutation, cx| mutation.mutate("b", cx));

        let first = finishes.borrow_mut().remove(0);
        first.send(Err(QueryError::msg("Stale."))).unwrap();
        cx.run_until_parked();
        mutation.read_with(cx, |mutation, _| {
            assert!(mutation.is_pending());
            assert!(mutation.error().is_none());
        });

        let last = finishes.borrow_mut().remove(0);
        last.send(Ok(2)).unwrap();
        cx.run_until_parked();
        assert!(!mutation.read_with(cx, |mutation, _| mutation.is_pending()));
        assert_eq!(second.now_or_never().unwrap().unwrap().unwrap(), 2);
    }
}
