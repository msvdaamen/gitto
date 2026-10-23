//! A view's hold on a query: TanStack Query's `useQuery`, with `select`.
//!
//! A [`Query`] lives in the view that shows it, made in its constructor (or again when what it's
//! of changes, like the repository): it loads the query if it's stale, keeps it active (loaded
//! again when it's invalidated) while the view keeps it, and re-renders the view when what it
//! shows changes, and only then: its selection, status, error or whether it's loading. Dropping
//! it lets the query go inactive; a load under way finishes, for whoever shows it next.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Instant;

use futures::channel::oneshot;
use gpui_kit::{App, Context, Entity, Subscription};

use super::client::{QueryClient, QueryEntry, QueryError, QueryOptions, QueryStatus};

/// What a view sees of a query: its selection (`S`, all of `T` by default), and its state.
pub struct Query<T: 'static, S: 'static = T> {
    entry: Entity<QueryEntry<T>>,
    snapshot: Rc<RefCell<Snapshot<S>>>,
    enabled: Rc<Cell<bool>>,
    _subscription: Subscription,
}

/// What's last been seen of the query: a change to any of it re-renders the view.
#[derive(Clone, PartialEq)]
struct Snapshot<S> {
    data: Option<S>,
    error: Option<QueryError>,
    status: QueryStatus,
    is_fetching: bool,
    data_updated_at: Option<Instant>,
}

impl<S> Snapshot<S> {
    fn of<T: Clone + PartialEq + Send + 'static>(
        entry: &QueryEntry<T>,
        select: &dyn Fn(&T) -> S,
    ) -> Self {
        Snapshot {
            data: entry.data().map(select),
            error: entry.error().cloned(),
            status: entry.status(),
            is_fetching: entry.is_fetching(),
            data_updated_at: entry.data_updated_at(),
        }
    }
}

impl<T: Clone + PartialEq + Send + 'static> Query<T> {
    /// Shows the query `options` in the view `cx` is of (see the module's doc).
    pub fn new<V: 'static>(options: QueryOptions<T>, cx: &mut Context<V>) -> Self {
        Query::with_select(options, T::clone, cx)
    }
}

impl<T: Clone + PartialEq + Send + 'static, S: Clone + PartialEq + 'static> Query<T, S> {
    /// Shows what `select` picks of the query `options` (TanStack's `select`): the view only
    /// renders again when that changes. Each view keeps its own selection, so pick what's shown,
    /// cheaply: a summary, or an `Opaque` that's shared rather than copied.
    pub fn with_select<V: 'static>(
        options: QueryOptions<T>,
        select: impl Fn(&T) -> S + 'static,
        cx: &mut Context<V>,
    ) -> Self {
        let entry = QueryClient::entry::<T>(options.key(), cx);
        let enabled = Rc::new(Cell::new(options.enabled));
        let now = QueryClient::now(cx);
        entry.update(cx, |entry, cx| {
            entry.set_options(&options);
            entry.observe(Rc::clone(&enabled));
            // Shown, it's loaded if it's stale (TanStack's `refetchOnMount`), unless it's loading.
            if options.enabled && entry.is_stale(now) && !entry.is_fetching() {
                entry.fetch(cx);
            }
        });

        let select: Rc<dyn Fn(&T) -> S> = Rc::new(select);
        let snapshot = Rc::new(RefCell::new(Snapshot::of(entry.read(cx), &*select)));
        let subscription = {
            let snapshot = Rc::clone(&snapshot);
            cx.observe(&entry, move |_, entry, cx| {
                let next = Snapshot::of(entry.read(cx), &*select);
                if *snapshot.borrow() != next {
                    *snapshot.borrow_mut() = next;
                    cx.notify();
                }
            })
        };
        Query {
            entry,
            snapshot,
            enabled,
            _subscription: subscription,
        }
    }
}

impl<T: Clone + PartialEq + Send + 'static, S: Clone + 'static> Query<T, S> {
    /// What's selected of the data; `None` until it's loaded. The data from before stays while
    /// it loads again, or if that fails.
    pub fn data(&self) -> Option<S> {
        self.snapshot.borrow().data.clone()
    }

    /// Why the last load failed, if it did.
    pub fn error(&self) -> Option<QueryError> {
        self.snapshot.borrow().error.clone()
    }

    pub fn status(&self) -> QueryStatus {
        self.snapshot.borrow().status
    }

    /// Nothing loaded yet, and no error.
    pub fn is_pending(&self) -> bool {
        self.status() == QueryStatus::Pending
    }

    pub fn is_error(&self) -> bool {
        self.status() == QueryStatus::Error
    }

    pub fn is_success(&self) -> bool {
        self.status() == QueryStatus::Success
    }

    /// Whether it's loading, the first time or again.
    pub fn is_fetching(&self) -> bool {
        self.snapshot.borrow().is_fetching
    }

    /// Loading for the first time: nothing to show yet.
    pub fn is_loading(&self) -> bool {
        self.is_fetching() && self.snapshot.borrow().data.is_none()
    }

    /// Loading again, with the data from before on show (what `UpdatingIndicator` says).
    pub fn is_refetching(&self) -> bool {
        self.is_fetching() && self.snapshot.borrow().data.is_some()
    }

    /// When it last loaded.
    pub fn data_updated_at(&self) -> Option<Instant> {
        self.snapshot.borrow().data_updated_at
    }

    /// Turns loading on or off (TanStack's `enabled`, changed): turned on, it loads if stale.
    pub fn set_enabled(&self, enabled: bool, cx: &mut App) {
        if self.enabled.replace(enabled) == enabled || !enabled {
            return;
        }
        let now = QueryClient::now(cx);
        self.entry.update(cx, |entry, cx| {
            if entry.is_stale(now) && !entry.is_fetching() {
                entry.fetch(cx);
            }
        });
    }

    /// Loads it again now, cancelling a load under way (TanStack's `refetch`); resolves once
    /// it's done. The result is read from the query, as it re-renders the view.
    pub fn refetch(&self, cx: &mut App) -> impl std::future::Future<Output = ()> + use<T, S> {
        let wait: Option<oneshot::Receiver<()>> =
            self.entry.update(cx, |entry, cx| entry.fetch(cx));
        async move {
            if let Some(wait) = wait {
                let _ = wait.await;
            }
        }
    }

    /// The query's cache entry, e.g. to observe it some other way.
    pub fn entry(&self) -> &Entity<QueryEntry<T>> {
        &self.entry
    }
}
