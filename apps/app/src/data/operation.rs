//! Changing a repository: running an operation (a pull, say), its state, and loading what it
//! changed. Ported from `git/queries/operation.ts`, TanStack's `useMutation` as the UI used it.
//!
//! Each repository's operations are kept in [`Operations`], a GPUI global, rather than in a view:
//! whether one is running, and why the last one failed, until that's dismissed or it's run again.
//! So they outlive the view showing them: switching repositories shows another one's, and the same
//! one's again when switching back. Views that show an operation observe the global
//! (`cx.observe_global::<Operations>(|_, cx| cx.notify())`).

use std::collections::HashMap;
use std::future::Future;
use std::rc::Rc;

use futures::channel::oneshot;
use gitto_git::input::RepositoryId;
use gpui_kit::{App, AppContext as _, AsyncApp, BorrowAppContext as _, Global, SharedString, Task};

use super::client::{QueryClient, QueryError};
use super::keys;

/// Whether an operation is running on a repository, and why it last failed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OperationState {
    pub running: bool,
    pub error: Option<QueryError>,
}

impl OperationState {
    /// Whether it's running (TanStack's `isPending`).
    pub fn is_pending(&self) -> bool {
        self.running
    }
}

/// Every repository's operations, by name and repository (see the module's doc).
#[derive(Default)]
pub struct Operations {
    states: HashMap<(SharedString, String), OperationState>,
}

impl Global for Operations {}

impl Operations {
    /// The operation `name`'s state on the repository `id`: idle, without an error, if it's not
    /// been run, or went through (`useRepositoryOperationState`).
    pub fn state(name: &str, id: &str, cx: &App) -> OperationState {
        cx.try_global::<Operations>()
            .and_then(|operations| {
                operations
                    .states
                    .get(&(SharedString::new(name), id.to_owned()))
            })
            .cloned()
            .unwrap_or_default()
    }

    /// Forgets why the operation `name` last failed on the repository `id`, unless it's running.
    pub fn dismiss(name: &str, id: &str, cx: &mut App) {
        let key = (SharedString::new(name), id.to_owned());
        if Operations::state(name, id, cx).running {
            return;
        }
        if cx.global::<Operations>().states.contains_key(&key) {
            cx.update_global::<Operations, _>(|operations, _| {
                operations.states.remove(&key);
            });
        }
    }

    fn set(name: &SharedString, id: &str, state: Option<OperationState>, cx: &mut App) {
        let key = (name.clone(), id.to_owned());
        cx.update_global::<Operations, _>(|operations, _| match state {
            Some(state) => {
                operations.states.insert(key, state);
            }
            None => {
                operations.states.remove(&key);
            }
        });
    }
}

/// Runs an operation's work on a repository, given its id and input, on the main thread; its
/// task runs the slow part on the background executor (see [`reloading`]).
pub type RunOperation<T, R> = Rc<dyn Fn(RepositoryId, T, &mut App) -> Task<Result<R, QueryError>>>;

/// An operation on a repository, by name, e.g. `pull` (`useRepositoryOperation`): one runs at a
/// time per repository, and [`Operations`] says whether it's running, and why it last failed.
/// Cheap to clone; a view makes one in its constructor.
pub struct RepositoryOperation<T, R = ()> {
    name: SharedString,
    run: RunOperation<T, R>,
}

impl<T, R> Clone for RepositoryOperation<T, R> {
    fn clone(&self) -> Self {
        RepositoryOperation {
            name: self.name.clone(),
            run: Rc::clone(&self.run),
        }
    }
}

impl<T: 'static, R: 'static> RepositoryOperation<T, R> {
    /// The operation `name`, which `run` does. `run` is given the repository's id rather than
    /// reading it when it ends, so the one it ran on is updated and reloaded even if another is
    /// on show by then.
    pub fn new(
        name: impl Into<SharedString>,
        run: impl Fn(RepositoryId, T, &mut App) -> Task<Result<R, QueryError>> + 'static,
    ) -> Self {
        RepositoryOperation {
            name: name.into(),
            run: Rc::new(run),
        }
    }

    pub fn name(&self) -> &SharedString {
        &self.name
    }

    /// Its state on the repository `id`.
    pub fn state(&self, id: &str, cx: &App) -> OperationState {
        Operations::state(&self.name, id, cx)
    }

    /// Forgets why it last failed on the repository `id`.
    pub fn dismiss(&self, id: &str, cx: &mut App) {
        Operations::dismiss(&self.name, id, cx)
    }

    /// Runs it on the repository `id`, unless it's running there: `None` then. It runs to the
    /// end whatever happens to the view that ran it; the receiver is told what it resolved to, for
    /// that view to act on if it's still there (TS's `onSuccess`).
    pub fn run(
        &self,
        id: &RepositoryId,
        input: T,
        cx: &mut App,
    ) -> Option<oneshot::Receiver<Result<R, QueryError>>> {
        if self.state(id, cx).running {
            return None;
        }
        let name = self.name.clone();
        let repository = id.to_string();
        Operations::set(
            &name,
            &repository,
            Some(OperationState {
                running: true,
                error: None,
            }),
            cx,
        );
        let work = (self.run)(id.clone(), input, cx);
        let (done, result) = oneshot::channel();
        cx.spawn(async move |cx| {
            let outcome = work.await;
            let state = match &outcome {
                // Nothing to keep for one that went through.
                Ok(_) => None,
                Err(error) => Some(OperationState {
                    running: false,
                    error: Some(error.clone()),
                }),
            };
            cx.update(|cx| Operations::set(&name, &repository, state, cx));
            let _ = done.send(outcome);
        })
        .detach();
        Some(result)
    }

    /// This operation, which doesn't run while `busy` says one of its group is: for operations
    /// that are shown together, like the stashes', so what's on show is never from before
    /// another (`oneAtATime`).
    pub fn one_at_a_time(self, busy: impl Fn(&str, &App) -> bool + 'static) -> OneAtATime<T, R> {
        OneAtATime {
            operation: self,
            busy: Rc::new(busy),
        }
    }
}

/// Whether one of an operation's group is running on a repository, by its id.
pub type Busy = Rc<dyn Fn(&str, &App) -> bool>;

/// An operation that waits for its group (see [`RepositoryOperation::one_at_a_time`]).
pub struct OneAtATime<T, R = ()> {
    operation: RepositoryOperation<T, R>,
    busy: Busy,
}

impl<T, R> Clone for OneAtATime<T, R> {
    fn clone(&self) -> Self {
        OneAtATime {
            operation: self.operation.clone(),
            busy: Rc::clone(&self.busy),
        }
    }
}

impl<T: 'static, R: 'static> OneAtATime<T, R> {
    pub fn operation(&self) -> &RepositoryOperation<T, R> {
        &self.operation
    }

    pub fn state(&self, id: &str, cx: &App) -> OperationState {
        self.operation.state(id, cx)
    }

    pub fn dismiss(&self, id: &str, cx: &mut App) {
        self.operation.dismiss(id, cx)
    }

    /// Runs it, unless it, or one of its group, is running.
    pub fn run(
        &self,
        id: &RepositoryId,
        input: T,
        cx: &mut App,
    ) -> Option<oneshot::Receiver<Result<R, QueryError>>> {
        if (self.busy)(id, cx) {
            return None;
        }
        self.operation.run(id, input, cx)
    }
}

/// Runs `action` (on the background executor) on the repository `id`, then reloads the
/// repository, also after a failure: the action may have changed it partway, e.g. a pop that
/// conflicts. Resolves once what's shown of it has loaded again.
pub async fn reloading<R, E>(
    id: &str,
    action: impl Future<Output = Result<R, E>> + Send + 'static,
    cx: &mut AsyncApp,
) -> Result<R, QueryError>
where
    R: Send + 'static,
    E: Into<QueryError> + Send + 'static,
{
    let result = cx.background_spawn(action).await;
    let key = keys::repository(id);
    cx.update(|cx| QueryClient::invalidate(&key, cx)).await;
    result.map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use futures::FutureExt as _;
    use gpui_kit::TestAppContext;

    use super::*;

    fn repository_id() -> RepositoryId {
        RepositoryId::parse(uuid::Uuid::now_v7().to_string()).unwrap()
    }

    fn init(cx: &mut TestAppContext) {
        cx.update(|cx| {
            QueryClient::init(cx);
            cx.set_global(Operations::default());
        });
    }

    #[gpui_kit::test]
    fn runs_one_at_a_time_per_repository_and_says_why_it_failed(cx: &mut TestAppContext) {
        init(cx);
        let (finish, finished) = futures::channel::oneshot::channel::<Result<(), QueryError>>();
        let finished = Rc::new(Cell::new(Some(finished)));
        let pull = RepositoryOperation::new("pull", move |_, (), cx| {
            let finished = finished.take().unwrap();
            cx.background_spawn(async move { finished.await.unwrap() })
        });
        let id = repository_id();

        let result = cx.update(|cx| pull.run(&id, (), cx)).unwrap();
        cx.run_until_parked();
        assert!(cx.update(|cx| pull.state(&id, cx).running));
        // Not again while it runs.
        assert!(cx.update(|cx| pull.run(&id, (), cx)).is_none());

        finish.send(Err(QueryError::msg("No upstream."))).unwrap();
        cx.run_until_parked();
        let state = cx.update(|cx| pull.state(&id, cx));
        assert!(!state.running);
        assert_eq!(state.error.unwrap().message(), "No upstream.");
        assert!(result.now_or_never().unwrap().unwrap().is_err());

        cx.update(|cx| pull.dismiss(&id, cx));
        assert_eq!(
            cx.update(|cx| pull.state(&id, cx)),
            OperationState::default()
        );
    }

    #[gpui_kit::test]
    fn forgets_one_that_went_through_and_reloads_the_repository(cx: &mut TestAppContext) {
        init(cx);
        let push = RepositoryOperation::new("push", |id: RepositoryId, (), cx: &mut App| {
            cx.spawn(async move |cx| reloading(&id, async { Ok::<_, QueryError>(7) }, cx).await)
        });
        let id = repository_id();

        let result = cx.update(|cx| push.run(&id, (), cx)).unwrap();
        cx.run_until_parked();
        assert_eq!(result.now_or_never().unwrap().unwrap().unwrap(), 7);
        assert_eq!(
            cx.update(|cx| push.state(&id, cx)),
            OperationState::default()
        );
        let invalidated = cx.update(|cx| cx.global::<QueryClient>().invalidated.clone());
        assert_eq!(invalidated, [keys::repository(&id)]);
    }

    #[gpui_kit::test]
    fn waits_for_its_group(cx: &mut TestAppContext) {
        init(cx);
        let busy = Rc::new(Cell::new(true));
        let drop_stash = RepositoryOperation::new("drop-stash", |_, (), cx: &mut App| {
            cx.background_spawn(async { Ok(()) })
        })
        .one_at_a_time({
            let busy = Rc::clone(&busy);
            move |_, _| busy.get()
        });
        let id = repository_id();

        assert!(cx.update(|cx| drop_stash.run(&id, (), cx)).is_none());
        busy.set(false);
        assert!(cx.update(|cx| drop_stash.run(&id, (), cx)).is_some());
    }
}
