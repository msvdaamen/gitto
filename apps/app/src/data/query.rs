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
    select: Rc<dyn Fn(&T) -> S>,
    snapshot: Rc<RefCell<Snapshot<S>>>,
    enabled: Rc<Cell<bool>>,
    _subscription: Subscription,
}

/// What's last been seen of the query: a change to any of it re-renders the view.
#[derive(Clone, PartialEq)]
struct Snapshot<S> {
    data: Option<S>,
    /// What was shown of the query before, while this one has nothing (see
    /// `QueryOptions::keep_previous_data`); dropped once it has.
    placeholder: Option<S>,
    error: Option<QueryError>,
    status: QueryStatus,
    is_fetching: bool,
    data_updated_at: Option<Instant>,
    updates: u64,
}

impl<S: Clone> Snapshot<S> {
    fn of<T: Clone + PartialEq + Send + 'static>(
        entry: &QueryEntry<T>,
        select: &dyn Fn(&T) -> S,
        placeholder: Option<S>,
    ) -> Self {
        let status = entry.status();
        Snapshot {
            data: entry.data().map(select),
            placeholder: placeholder.filter(|_| status == QueryStatus::Pending),
            error: entry.error().cloned(),
            status,
            is_fetching: entry.is_fetching(),
            data_updated_at: entry.data_updated_at(),
            updates: entry.updates(),
        }
    }

    /// The data, or the placeholder while there's none.
    fn shown(&self) -> Option<&S> {
        self.data.as_ref().or(self.placeholder.as_ref())
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
        Query::build(options, Rc::new(select), None, cx)
    }

    /// Shows the query `options` instead, with the same selection (TanStack's `useQuery` given
    /// another key): another commit's, say. Of the same query, it only takes on its options (and
    /// whether it's enabled). Of another, with `QueryOptions::keep_previous_data`, what was shown
    /// stays on show until the other has loaded (see [`is_placeholder_data`](Self::is_placeholder_data)).
    pub fn set_options<V: 'static>(&mut self, options: QueryOptions<T>, cx: &mut Context<V>) {
        if options.key() == self.entry.read(cx).key() {
            self.entry
                .update(cx, |entry, _| entry.set_options(&options));
            self.set_enabled(options.enabled, cx);
            return;
        }
        let placeholder = if options.keep_previous_data {
            self.snapshot.borrow().shown().cloned()
        } else {
            None
        };
        *self = Query::build(options, Rc::clone(&self.select), placeholder, cx);
        cx.notify();
    }

    fn build<V: 'static>(
        options: QueryOptions<T>,
        select: Rc<dyn Fn(&T) -> S>,
        placeholder: Option<S>,
        cx: &mut Context<V>,
    ) -> Self {
        let entry = QueryClient::entry::<T>(options.key(), cx);
        let enabled = Rc::new(Cell::new(options.enabled));
        let initial = match &options.initial_data {
            Some(initial) if entry.read(cx).data().is_none() => initial(cx),
            _ => None,
        };
        let now = QueryClient::now(cx);
        entry.update(cx, |entry, cx| {
            entry.set_options(&options);
            entry.observe(Rc::clone(&enabled));
            if let Some(initial) = initial {
                entry.set_data(initial, cx);
            }
            // Shown, it's loaded if it's stale (TanStack's `refetchOnMount`), unless it's loading.
            if options.enabled && entry.is_stale(now) && !entry.is_fetching() {
                entry.fetch(cx);
            }
        });

        let snapshot = Rc::new(RefCell::new(Snapshot::of(
            entry.read(cx),
            &*select,
            placeholder,
        )));
        let subscription = {
            let snapshot = Rc::clone(&snapshot);
            let select = Rc::clone(&select);
            cx.observe(&entry, move |_, entry, cx| {
                let placeholder = snapshot.borrow().placeholder.clone();
                let next = Snapshot::of(entry.read(cx), &*select, placeholder);
                if *snapshot.borrow() != next {
                    *snapshot.borrow_mut() = next;
                    cx.notify();
                }
            })
        };
        Query {
            entry,
            select,
            snapshot,
            enabled,
            _subscription: subscription,
        }
    }
}

impl<T: Clone + PartialEq + Send + 'static, S: Clone + 'static> Query<T, S> {
    /// What's selected of the data; `None` until it's loaded. The data from before stays while
    /// it loads again, or if that fails; so does another query's it was switched from, while this
    /// one has none (see [`is_placeholder_data`](Self::is_placeholder_data)).
    pub fn data(&self) -> Option<S> {
        self.snapshot.borrow().shown().cloned()
    }

    /// Whether [`data`](Self::data) is the query's it was switched from, as this one hasn't loaded
    /// yet (TanStack's `isPlaceholderData`; see `QueryOptions::keep_previous_data`).
    pub fn is_placeholder_data(&self) -> bool {
        let snapshot = self.snapshot.borrow();
        snapshot.data.is_none() && snapshot.placeholder.is_some()
    }

    /// Why the last load failed, if it did.
    pub fn error(&self) -> Option<QueryError> {
        self.snapshot.borrow().error.clone()
    }

    /// Where it is; [`QueryStatus::Success`] while it shows a placeholder, as TanStack has it.
    pub fn status(&self) -> QueryStatus {
        if self.is_placeholder_data() {
            return QueryStatus::Success;
        }
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

    /// Loading for the first time: nothing of it to show yet.
    pub fn is_loading(&self) -> bool {
        self.is_fetching() && self.snapshot.borrow().shown().is_none()
    }

    /// Loading again, with the data from before on show (what `UpdatingIndicator` says).
    pub fn is_refetching(&self) -> bool {
        self.is_fetching() && self.snapshot.borrow().shown().is_some()
    }

    /// When it last loaded.
    pub fn data_updated_at(&self) -> Option<Instant> {
        self.snapshot.borrow().data_updated_at
    }

    /// How many times it's been loaded or set (see `QueryEntry::updates`): changes whenever
    /// [`data_updated_at`](Self::data_updated_at) does.
    pub fn updates(&self) -> u64 {
        self.snapshot.borrow().updates
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
