//! The query cache: what's been loaded, by key, and keeping it fresh. TanStack Query's
//! `QueryClient` (`lib/query-client.ts`), the part of it the UI used.
//!
//! Each key has a [`QueryEntry`], an entity: its data, why it last failed, whether it's loading,
//! and how to load it. Views don't hold entries; they hold a `Query` (see `super::query`), which
//! observes one, and keeps it active while the view lives. [`QueryClient`] is a GPUI global that
//! finds entries by key, invalidates them by prefix, and refetches them when the window gets
//! focus.

use std::any::Any;
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::channel::oneshot;
use futures::future::{BoxFuture, join_all};
use gitto_git::{ApiCode, ApiError};
use gpui_kit::{App, AppContext as _, Context, Entity, Global, Task};

use super::key::QueryKey;

/// How long a query nothing shows is kept before it's dropped (TanStack's `gcTime`).
pub const GC_TIME: Duration = Duration::from_secs(5 * 60);

/// Why a query, or an operation, failed: shared, so every view showing it has the same one.
/// Usually git's [`ApiError`] (see [`QueryError::api`]).
#[derive(Clone)]
pub struct QueryError(Arc<dyn std::error::Error + Send + Sync>);

impl QueryError {
    pub fn new(error: impl std::error::Error + Send + Sync + 'static) -> Self {
        QueryError(Arc::new(error))
    }

    /// An error that's only its message, e.g. a test's.
    pub fn msg(message: impl Into<String>) -> Self {
        QueryError::new(Message(message.into()))
    }

    /// What the user is told.
    pub fn message(&self) -> String {
        self.0.to_string()
    }

    /// The error as git's API made it, with its code; `None` for one that isn't.
    pub fn api(&self) -> Option<&ApiError> {
        self.0.downcast_ref::<ApiError>()
    }

    /// Whether it's the API's error with `code` (`hasCode` in `lib/errors.ts`).
    pub fn has_code(&self, code: ApiCode) -> bool {
        self.api().is_some_and(|error| error.code() == code)
    }

    pub fn inner(&self) -> &(dyn std::error::Error + Send + Sync + 'static) {
        &*self.0
    }
}

/// The same error, not merely one that says the same.
impl PartialEq for QueryError {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl std::fmt::Debug for QueryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&*self.0, f)
    }
}

impl std::fmt::Display for QueryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&*self.0, f)
    }
}

impl From<ApiError> for QueryError {
    fn from(error: ApiError) -> Self {
        QueryError::new(error)
    }
}

impl From<gitto_repository::Error> for QueryError {
    fn from(error: gitto_repository::Error) -> Self {
        QueryError::new(error)
    }
}

impl From<std::io::Error> for QueryError {
    fn from(error: std::io::Error) -> Self {
        QueryError::new(error)
    }
}

/// With its causes (`{:#}`), which its `Display` alone leaves out.
impl From<anyhow::Error> for QueryError {
    fn from(error: anyhow::Error) -> Self {
        QueryError::msg(format!("{error:#}"))
    }
}

#[derive(Debug)]
struct Message(String);

impl std::fmt::Display for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Message {}

/// How long loaded data counts as fresh: until then, showing it again doesn't load it again
/// (TanStack's `staleTime`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaleTime {
    /// Loaded again whenever it's shown, and when the window gets focus (TanStack's default).
    Zero,
    After(Duration),
    /// Only loaded again once it's invalidated (`Infinity`).
    Infinite,
}

/// Whether a shown query is loaded again when the window gets focus (TanStack's
/// `refetchOnWindowFocus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefetchOnFocus {
    Never,
    /// If it's stale (TanStack's `true`, the default).
    IfStale,
    Always,
}

/// Loads a query: given what it had, if anything, on the main thread, with the app to read
/// services from; the future it returns runs on the background executor, and is dropped, which
/// cancels it, when the query's loaded again before it's done.
pub type Fetcher<T> = Rc<dyn Fn(Option<&T>, &App) -> BoxFuture<'static, Result<T, QueryError>>>;

/// A setting that may depend on the query's data, like `version.ts`'s stale time.
pub type BySetting<T, V> = Rc<dyn Fn(Option<&T>) -> V>;

/// Data a query starts with, read from what else is cached, e.g. a commit from the log (TanStack's
/// `initialData`); `None` if there's none.
pub type InitialData<T> = Rc<dyn Fn(&App) -> Option<T>>;

/// What a query is and how it loads (TanStack's query options). Built by each query's module
/// (`status_query`…), and given to `Query::new`.
pub struct QueryOptions<T> {
    pub(crate) key: QueryKey,
    pub(crate) fetch: Fetcher<T>,
    pub(crate) enabled: bool,
    pub(crate) stale_time: Option<BySetting<T, StaleTime>>,
    pub(crate) refetch_on_window_focus: Option<BySetting<T, RefetchOnFocus>>,
    pub(crate) gc_time: Option<Duration>,
    pub(crate) keep_previous_data: bool,
    pub(crate) initial_data: Option<InitialData<T>>,
}

impl<T: 'static> QueryOptions<T> {
    /// The query `key`, loaded by `fetch` (see [`Fetcher`]).
    pub fn new(
        key: QueryKey,
        fetch: impl Fn(Option<&T>, &App) -> BoxFuture<'static, Result<T, QueryError>> + 'static,
    ) -> Self {
        QueryOptions {
            key,
            fetch: Rc::new(fetch),
            enabled: true,
            stale_time: None,
            refetch_on_window_focus: None,
            gc_time: None,
            keep_previous_data: false,
            initial_data: None,
        }
    }

    pub fn key(&self) -> &QueryKey {
        &self.key
    }

    /// Whether it loads at all; a disabled query only shows what's cached (TanStack's
    /// `enabled`).
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Overrides the client's default for its key (see [`QueryClient::set_query_defaults`]).
    pub fn stale_time(self, stale_time: StaleTime) -> Self {
        self.stale_time_with(move |_| stale_time)
    }

    /// The stale time, by the data loaded: e.g. never stale once git is supported.
    pub fn stale_time_with(
        mut self,
        stale_time: impl Fn(Option<&T>) -> StaleTime + 'static,
    ) -> Self {
        self.stale_time = Some(Rc::new(stale_time));
        self
    }

    pub fn refetch_on_window_focus(self, refetch: RefetchOnFocus) -> Self {
        self.refetch_on_window_focus_with(move |_| refetch)
    }

    pub fn refetch_on_window_focus_with(
        mut self,
        refetch: impl Fn(Option<&T>) -> RefetchOnFocus + 'static,
    ) -> Self {
        self.refetch_on_window_focus = Some(Rc::new(refetch));
        self
    }

    /// How long it's kept once nothing shows it, rather than [`GC_TIME`] (TanStack's `gcTime`):
    /// shorter for what's big and quick to load again, like a file's contents.
    pub fn gc_time(mut self, gc_time: Duration) -> Self {
        self.gc_time = Some(gc_time);
        self
    }

    /// While a `Query` switched to this one (`Query::set_options`) has nothing loaded of it, it
    /// shows what it showed before, as `Query::is_placeholder_data` says, rather than nothing
    /// (TanStack's `placeholderData: keepPreviousData`): what's on show stays until the next is in.
    pub fn keep_previous_data(mut self) -> Self {
        self.keep_previous_data = true;
        self
    }

    /// What it starts with when nothing's cached for it, if `initial` finds it elsewhere (TanStack's
    /// `initialData`): it counts as loaded just now, so with a stale time it isn't loaded again.
    pub fn initial_data(mut self, initial: impl Fn(&App) -> Option<T> + 'static) -> Self {
        self.initial_data = Some(Rc::new(initial));
        self
    }
}

/// Defaults for every query under a key (TanStack's `setQueryDefaults`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueryDefaults {
    pub stale_time: StaleTime,
    pub refetch_on_window_focus: RefetchOnFocus,
}

impl Default for QueryDefaults {
    /// TanStack's: loaded again whenever shown, and on focus. (Its retries are off, as
    /// `query-client.ts` turned them off: git's errors don't go away by retrying.)
    fn default() -> Self {
        QueryDefaults {
            stale_time: StaleTime::Zero,
            refetch_on_window_focus: RefetchOnFocus::IfStale,
        }
    }
}

/// Where a query is (TanStack's `status`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryStatus {
    /// Nothing loaded yet.
    Pending,
    /// The last load failed; the data from before, if any, is kept.
    Error,
    Success,
}

/// One query's cache entry (see the module's doc). Its data is compared with what it loads
/// again, and kept, untouched, when they're equal, so nothing that shows it renders again
/// (TanStack's structural sharing; see `super::opaque` for data too big to compare).
pub struct QueryEntry<T> {
    key: QueryKey,
    data: Option<T>,
    error: Option<QueryError>,
    data_updated_at: Option<Instant>,
    /// How many times it's been loaded or set (see `updates`).
    updates: u64,
    is_fetching: bool,
    is_invalidated: bool,
    fetch: Option<Fetcher<T>>,
    stale_time: Option<BySetting<T, StaleTime>>,
    refetch_on_window_focus: Option<BySetting<T, RefetchOnFocus>>,
    defaults: QueryDefaults,
    /// How long it's kept once nothing shows it.
    gc_time: Duration,
    /// The load under way: dropped to cancel it.
    task: Option<Task<()>>,
    /// Told once the load under way is done, whichever replaced which.
    waiters: Vec<oneshot::Sender<()>>,
    /// The `Query`s showing it, each with whether it's enabled.
    observers: Vec<Weak<std::cell::Cell<bool>>>,
}

impl<T: Clone + PartialEq + Send + 'static> QueryEntry<T> {
    fn new(key: QueryKey, defaults: QueryDefaults) -> Self {
        QueryEntry {
            key,
            data: None,
            error: None,
            data_updated_at: None,
            updates: 0,
            is_fetching: false,
            is_invalidated: false,
            fetch: None,
            stale_time: None,
            refetch_on_window_focus: None,
            defaults,
            gc_time: GC_TIME,
            task: None,
            waiters: Vec::new(),
            observers: Vec::new(),
        }
    }

    pub fn key(&self) -> &QueryKey {
        &self.key
    }

    pub fn data(&self) -> Option<&T> {
        self.data.as_ref()
    }

    pub fn error(&self) -> Option<&QueryError> {
        self.error.as_ref()
    }

    pub fn status(&self) -> QueryStatus {
        if self.error.is_some() {
            QueryStatus::Error
        } else if self.data.is_some() {
            QueryStatus::Success
        } else {
            QueryStatus::Pending
        }
    }

    pub fn is_fetching(&self) -> bool {
        self.is_fetching
    }

    pub fn is_invalidated(&self) -> bool {
        self.is_invalidated
    }

    /// When it last loaded, by the background executor's clock (see `QueryClient::now`).
    pub fn data_updated_at(&self) -> Option<Instant> {
        self.data_updated_at
    }

    /// How many times it's been loaded or set, equal data or not: what changes with
    /// `data_updated_at`, to tell whether it's been loaded since, whatever the clock says.
    pub fn updates(&self) -> u64 {
        self.updates
    }

    /// Takes on `options`' way of loading, as the latest `Query` to show it gives them: their
    /// settings, or the client's defaults for the ones they leave out, so what an earlier `Query`
    /// set (an unwatched one's stale time, say) doesn't stick to a watched one of the same key.
    pub(crate) fn set_options(&mut self, options: &QueryOptions<T>) {
        self.fetch = Some(options.fetch.clone());
        self.stale_time = options.stale_time.clone();
        self.refetch_on_window_focus = options.refetch_on_window_focus.clone();
        if let Some(gc_time) = options.gc_time {
            self.gc_time = gc_time;
        }
    }

    pub(crate) fn observe(&mut self, enabled: Rc<std::cell::Cell<bool>>) {
        self.observers
            .retain(|observer| observer.strong_count() > 0);
        self.observers.push(Rc::downgrade(&enabled));
    }

    /// Whether an enabled `Query` shows it (TanStack's "active").
    pub fn is_active(&self) -> bool {
        self.observers
            .iter()
            .any(|observer| observer.upgrade().is_some_and(|enabled| enabled.get()))
    }

    fn is_observed(&self) -> bool {
        self.observers
            .iter()
            .any(|observer| observer.strong_count() > 0)
    }

    /// Whether it's to be loaded again when it's shown: it's invalidated, has nothing, or has
    /// been loaded longer ago than its stale time.
    pub fn is_stale(&self, now: Instant) -> bool {
        let Some(updated_at) = self.data_updated_at else {
            return true;
        };
        if self.is_invalidated || self.data.is_none() {
            return true;
        }
        match self.stale_time() {
            StaleTime::Zero => true,
            StaleTime::After(duration) => now.duration_since(updated_at) >= duration,
            StaleTime::Infinite => false,
        }
    }

    fn stale_time(&self) -> StaleTime {
        self.stale_time
            .as_ref()
            .map_or(self.defaults.stale_time, |by| by(self.data.as_ref()))
    }

    fn refetch_on_focus(&self) -> RefetchOnFocus {
        self.refetch_on_window_focus
            .as_ref()
            .map_or(self.defaults.refetch_on_window_focus, |by| {
                by(self.data.as_ref())
            })
    }

    /// Loads it, replacing (and so cancelling) a load under way; a receiver told once it's done,
    /// or `None` if it has no way to load yet.
    pub fn fetch(&mut self, cx: &mut Context<Self>) -> Option<oneshot::Receiver<()>> {
        let fetch = self.fetch.clone()?;
        let (done, wait) = oneshot::channel();
        self.waiters.push(done);
        // What's loading for the first time isn't loaded twice: there's nothing on show to
        // replace, so the load under way is waited for (as TanStack does).
        if self.is_fetching && self.data.is_none() {
            return Some(wait);
        }
        let load = fetch(self.data.as_ref(), cx);
        let load = cx.background_spawn(load);
        self.is_fetching = true;
        self.task = Some(cx.spawn(async move |this, cx| {
            let result = load.await;
            let _ = this.update(cx, |entry, cx| entry.settle(result, cx));
        }));
        cx.notify();
        Some(wait)
    }

    /// A receiver told once the load under way is done; `None` if none is.
    pub fn wait(&mut self) -> Option<oneshot::Receiver<()>> {
        if !self.is_fetching {
            return None;
        }
        let (done, wait) = oneshot::channel();
        self.waiters.push(done);
        Some(wait)
    }

    /// Stops the load under way, if any, leaving the data as it was (TanStack's `cancelQueries`):
    /// e.g. so one that started before a change can't overwrite what the change set.
    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        if !self.is_fetching {
            return;
        }
        self.task = None;
        self.is_fetching = false;
        for waiter in self.waiters.drain(..) {
            let _ = waiter.send(());
        }
        cx.notify();
    }

    fn settle(&mut self, result: Result<T, QueryError>, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        match result {
            Ok(data) => {
                // Equal data stays as it was: what's selected from it is the same, and nothing
                // renders again.
                if self.data.as_ref() != Some(&data) {
                    self.data = Some(data);
                }
                self.error = None;
                self.data_updated_at = Some(now);
                self.updates += 1;
                self.is_invalidated = false;
            }
            Err(error) => self.error = Some(error),
        }
        self.is_fetching = false;
        self.task = None;
        for waiter in self.waiters.drain(..) {
            let _ = waiter.send(());
        }
        cx.notify();
    }

    /// Sets its data, as if it had loaded it (TanStack's `setQueryData`): fresh, so no longer
    /// invalidated.
    pub fn set_data(&mut self, data: T, cx: &mut Context<Self>) {
        self.data = Some(data);
        self.error = None;
        self.data_updated_at = Some(cx.background_executor().now());
        self.updates += 1;
        self.is_invalidated = false;
        cx.notify();
    }
}

/// An entry of any type, for what the client does to every entry under a key.
trait AnyQueryEntry {
    fn as_any(&self) -> &dyn Any;
    fn info(&self, cx: &App) -> EntryInfo;
    /// Marks it invalidated, and loads it again if it's shown; told once that's done.
    fn invalidate(&self, cx: &mut App) -> Option<oneshot::Receiver<()>>;
    fn refetch_on_focus(&self, now: Instant, cx: &mut App);
    fn cancel(&self, cx: &mut App);
}

/// What a predicate given to `QueryClient::invalidate_where` sees of an entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryInfo {
    pub key: QueryKey,
    pub data_updated_at: Option<Instant>,
    pub updates: u64,
    pub is_active: bool,
    pub is_fetching: bool,
    pub is_observed: bool,
    /// How long it's kept once nothing shows it.
    pub gc_time: Duration,
}

impl<T: Clone + PartialEq + Send + 'static> AnyQueryEntry for Entity<QueryEntry<T>> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn info(&self, cx: &App) -> EntryInfo {
        let entry = self.read(cx);
        EntryInfo {
            key: entry.key.clone(),
            data_updated_at: entry.data_updated_at,
            updates: entry.updates,
            is_active: entry.is_active(),
            is_fetching: entry.is_fetching,
            is_observed: entry.is_observed(),
            gc_time: entry.gc_time,
        }
    }

    fn invalidate(&self, cx: &mut App) -> Option<oneshot::Receiver<()>> {
        self.update(cx, |entry, cx| {
            entry.is_invalidated = true;
            if entry.is_active() {
                entry.fetch(cx)
            } else {
                cx.notify();
                None
            }
        })
    }

    fn refetch_on_focus(&self, now: Instant, cx: &mut App) {
        self.update(cx, |entry, cx| {
            if !entry.is_active() {
                return;
            }
            let refetch = match entry.refetch_on_focus() {
                RefetchOnFocus::Never => false,
                RefetchOnFocus::IfStale => entry.is_stale(now),
                RefetchOnFocus::Always => true,
            };
            if refetch {
                entry.fetch(cx);
            }
        });
    }

    fn cancel(&self, cx: &mut App) {
        self.update(cx, |entry, cx| entry.cancel(cx));
    }
}

/// The loads an invalidation started: `.await` it to wait for them, or drop it not to (they run
/// either way).
pub struct Reloading(Vec<oneshot::Receiver<()>>);

impl std::future::IntoFuture for Reloading {
    type Output = ();
    type IntoFuture = BoxFuture<'static, ()>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            join_all(self.0).await;
        })
    }
}

/// The query cache (see the module's doc): a GPUI global, set up by [`QueryClient::init`].
#[derive(Default)]
pub struct QueryClient {
    entries: HashMap<QueryKey, Rc<dyn AnyQueryEntry>>,
    defaults: Vec<(QueryKey, QueryDefaults)>,
    /// Since when each query nothing shows has been seen so (see `collect_garbage`).
    inactive: HashMap<QueryKey, Instant>,
    /// The keys invalidated, in order, for tests to check (TS's spy on `invalidateQueries`).
    #[cfg(test)]
    pub(crate) invalidated: Vec<QueryKey>,
}

impl Global for QueryClient {}

impl QueryClient {
    /// Sets up the cache, with Gitto's defaults (`lib/query-client.ts`): what's loaded from a
    /// repository is loaded again when the repository changes on disk (see `super::watch`), and
    /// only then: not also whenever something that shows it is rendered, like the uncommitted
    /// changes' row scrolling back into view, which would run `git status` again, or when the
    /// window gets focus.
    pub fn init(cx: &mut App) {
        let mut client = QueryClient::default();
        client.set_query_defaults(
            super::keys::all(),
            QueryDefaults {
                stale_time: StaleTime::Infinite,
                refetch_on_window_focus: RefetchOnFocus::Never,
            },
        );
        cx.set_global(client);
    }

    /// Sets the defaults for every query under `prefix`; the longest prefix wins.
    pub fn set_query_defaults(&mut self, prefix: QueryKey, defaults: QueryDefaults) {
        self.defaults.retain(|(key, _)| *key != prefix);
        self.defaults.push((prefix, defaults));
    }

    fn defaults_for(&self, key: &QueryKey) -> QueryDefaults {
        self.defaults
            .iter()
            .filter(|(prefix, _)| key.starts_with(prefix))
            .max_by_key(|(prefix, _)| prefix.segments().len())
            .map_or_else(QueryDefaults::default, |(_, defaults)| *defaults)
    }

    /// The time by the background executor's clock, which tests advance.
    pub fn now(cx: &App) -> Instant {
        cx.background_executor().now()
    }

    /// The entry for `key`, made if there's none. Panics if `key` holds another type's.
    pub fn entry<T: Clone + PartialEq + Send + 'static>(
        key: &QueryKey,
        cx: &mut App,
    ) -> Entity<QueryEntry<T>> {
        Self::collect_garbage(cx);
        if let Some(entry) = cx.global::<QueryClient>().entries.get(key) {
            return entry
                .as_any()
                .downcast_ref::<Entity<QueryEntry<T>>>()
                .unwrap_or_else(|| panic!("the query {key:?} holds another type"))
                .clone();
        }
        let defaults = cx.global::<QueryClient>().defaults_for(key);
        let entry = cx.new(|_| QueryEntry::<T>::new(key.clone(), defaults));
        cx.global_mut::<QueryClient>()
            .entries
            .insert(key.clone(), Rc::new(entry.clone()));
        entry
    }

    /// The entry for `key`, if there's one.
    pub fn get_entry<T: Clone + PartialEq + Send + 'static>(
        key: &QueryKey,
        cx: &App,
    ) -> Option<Entity<QueryEntry<T>>> {
        cx.global::<QueryClient>()
            .entries
            .get(key)?
            .as_any()
            .downcast_ref::<Entity<QueryEntry<T>>>()
            .cloned()
    }

    /// What's cached for `key` (TanStack's `getQueryData`).
    pub fn get_data<T: Clone + PartialEq + Send + 'static>(key: &QueryKey, cx: &App) -> Option<T> {
        Self::get_entry::<T>(key, cx)?.read(cx).data.clone()
    }

    /// Caches `data` for `key`, as if it had loaded (TanStack's `setQueryData`).
    pub fn set_data<T: Clone + PartialEq + Send + 'static>(key: &QueryKey, data: T, cx: &mut App) {
        Self::entry::<T>(key, cx).update(cx, |entry, cx| entry.set_data(data, cx));
    }

    /// Changes what's cached for `key` with `update`, if anything is; whether it did (TanStack's
    /// `setQueryData` with an updater that keeps `undefined`).
    pub fn update_data<T: Clone + PartialEq + Send + 'static>(
        key: &QueryKey,
        update: impl FnOnce(&T) -> T,
        cx: &mut App,
    ) -> bool {
        let Some(entry) = Self::get_entry::<T>(key, cx) else {
            return false;
        };
        let Some(data) = entry.read(cx).data.as_ref().map(update) else {
            return false;
        };
        entry.update(cx, |entry, cx| entry.set_data(data, cx));
        true
    }

    /// How many times what's cached for `key` has been loaded or set (see `QueryEntry::updates`);
    /// 0 if nothing is.
    pub fn updates(key: &QueryKey, cx: &App) -> u64 {
        cx.global::<QueryClient>()
            .entries
            .get(key)
            .map_or(0, |entry| entry.info(cx).updates)
    }

    /// The query `options`' data: what's cached, if it's fresh, or else what it loads (TanStack's
    /// `fetchQuery`), e.g. to load what's about to be shown ahead of it. A load under way is waited
    /// for rather than started again. What it loads is cached as any query's, and dropped once
    /// nothing has shown it for its gc time.
    pub fn fetch_query<T: Clone + PartialEq + Send + 'static>(
        options: QueryOptions<T>,
        cx: &mut App,
    ) -> Task<Result<T, QueryError>> {
        let entry = Self::entry::<T>(options.key(), cx);
        let now = Self::now(cx);
        let wait = entry.update(cx, |entry, cx| {
            entry.set_options(&options);
            if entry.data.is_some() && !entry.is_stale(now) {
                None
            } else if entry.is_fetching {
                entry.wait()
            } else {
                entry.fetch(cx)
            }
        });
        cx.spawn(async move |cx| {
            let loaded = wait.is_some();
            if let Some(wait) = wait {
                let _ = wait.await;
            }
            cx.update(|cx| {
                let entry = entry.read(cx);
                match (&entry.error, &entry.data) {
                    // Fresh: what's cached, even if a load since failed.
                    (_, Some(data)) if !loaded => Ok(data.clone()),
                    (Some(error), _) => Err(error.clone()),
                    (None, Some(data)) => Ok(data.clone()),
                    (None, None) => Err(QueryError::msg("The query was cancelled.")),
                }
            })
        })
    }

    /// Stops the load under way of the query `key`, exactly, if there is one (see
    /// [`QueryEntry::cancel`]).
    pub fn cancel(key: &QueryKey, cx: &mut App) {
        let entry = cx.global::<QueryClient>().entries.get(key).cloned();
        if let Some(entry) = entry {
            entry.cancel(cx);
        }
    }

    /// Marks every query under `prefix` invalidated, and loads again those that are shown; the
    /// others load once they're shown again (TanStack's `invalidateQueries`). The future resolves
    /// once those loads are done.
    pub fn invalidate(prefix: &QueryKey, cx: &mut App) -> Reloading {
        Self::invalidate_where(prefix, |_| true, cx)
    }

    /// [`invalidate`](Self::invalidate), for the queries under `prefix` that `predicate` picks.
    pub fn invalidate_where(
        prefix: &QueryKey,
        predicate: impl Fn(&EntryInfo) -> bool,
        cx: &mut App,
    ) -> Reloading {
        #[cfg(test)]
        cx.global_mut::<QueryClient>()
            .invalidated
            .push(prefix.clone());
        let entries: Vec<_> = cx
            .global::<QueryClient>()
            .entries
            .iter()
            .filter(|(key, _)| key.starts_with(prefix))
            .map(|(_, entry)| Rc::clone(entry))
            .collect();
        let picked: Vec<_> = entries
            .into_iter()
            .filter(|entry| predicate(&entry.info(cx)))
            .collect();
        let waits = picked
            .into_iter()
            .filter_map(|entry| entry.invalidate(cx))
            .collect();
        Reloading(waits)
    }

    /// Loads again the shown queries that load on focus (see [`RefetchOnFocus`]): call when the
    /// window gets focus.
    pub fn window_focused(cx: &mut App) {
        let now = Self::now(cx);
        let entries: Vec<_> = cx
            .global::<QueryClient>()
            .entries
            .values()
            .cloned()
            .collect();
        for entry in entries {
            entry.refetch_on_focus(now, cx);
        }
    }

    /// Drops every query (TanStack's `clear`).
    pub fn clear(cx: &mut App) {
        let client = cx.global_mut::<QueryClient>();
        client.entries.clear();
        client.inactive.clear();
    }

    /// Drops the queries nothing has shown for their gc time ([`GC_TIME`] by default): noticed here, as `Query`s go without
    /// telling their entry.
    fn collect_garbage(cx: &mut App) {
        let now = Self::now(cx);
        let entries: Vec<_> = cx
            .global::<QueryClient>()
            .entries
            .iter()
            .map(|(key, entry)| (key.clone(), Rc::clone(entry)))
            .collect();
        let mut expired = Vec::new();
        for (key, entry) in entries {
            let info = entry.info(cx);
            if info.is_observed || info.is_fetching {
                cx.global_mut::<QueryClient>().inactive.remove(&key);
                continue;
            }
            // Its first time seen unobserved starts the clock.
            if !Self::mark_inactive(&key, now, info.gc_time, cx) {
                continue;
            }
            expired.push(key);
        }
        let client = cx.global_mut::<QueryClient>();
        for key in expired {
            client.entries.remove(&key);
            client.inactive.remove(&key);
        }
    }

    /// Whether `entry`, unobserved, has been for `gc_time`; starts timing it if it wasn't.
    fn mark_inactive(key: &QueryKey, now: Instant, gc_time: Duration, cx: &mut App) -> bool {
        let since = cx
            .global_mut::<QueryClient>()
            .inactive
            .entry(key.clone())
            .or_insert(now);
        now.duration_since(*since) >= gc_time
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use futures::FutureExt as _;
    use gpui_kit::{AppContext as _, TestAppContext};

    use super::*;
    use crate::data::query::Query;

    fn repo() -> gitto_git::input::RepositoryId {
        gitto_git::input::RepositoryId::parse("01890a5d-ac96-774b-bcce-b302099a8057").unwrap()
    }

    /// A view showing a query.
    struct Probe {
        _query: Query<i32>,
    }

    /// The status's query (under `keys::repository`), counting its loads.
    fn options(loads: &Rc<Cell<usize>>) -> QueryOptions<i32> {
        let loads = Rc::clone(loads);
        QueryOptions::new(crate::data::keys::status(&repo()), move |_, _| {
            loads.set(loads.get() + 1);
            async { Ok(1) }.boxed()
        })
    }

    // `lib/query-client.test.ts`.

    #[gpui_kit::test]
    fn a_repositorys_data_isnt_loaded_again_when_something_else_that_shows_it_is_rendered(
        cx: &mut TestAppContext,
    ) {
        cx.update(QueryClient::init);
        let loads = Rc::new(Cell::new(0));
        let _first = cx.new(|cx| Probe {
            _query: Query::new(options(&loads), cx),
        });
        cx.run_until_parked();
        let _second = cx.new(|cx| Probe {
            _query: Query::new(options(&loads), cx),
        });
        cx.run_until_parked();
        assert_eq!(loads.get(), 1);
    }

    #[gpui_kit::test]
    fn a_repositorys_data_is_loaded_again_once_its_said_to_have_changed_even_if_nothing_showed_it(
        cx: &mut TestAppContext,
    ) {
        cx.update(QueryClient::init);
        let loads = Rc::new(Cell::new(0));
        let first = cx.new(|cx| Probe {
            _query: Query::new(options(&loads), cx),
        });
        cx.run_until_parked();
        drop(first);
        cx.update(|_| {});

        cx.update(|cx| {
            let _ = QueryClient::invalidate(&crate::data::keys::repository(&repo()), cx);
        });
        cx.run_until_parked();
        assert_eq!(loads.get(), 1);
        let _second = cx.new(|cx| Probe {
            _query: Query::new(options(&loads), cx),
        });
        cx.run_until_parked();
        assert_eq!(loads.get(), 2);
    }

    type Answers = Rc<std::cell::RefCell<Vec<oneshot::Sender<Result<String, QueryError>>>>>;

    /// A query of a commit's (outside `keys::repository`), loaded with what `answers` gives: its
    /// answers, in turn, when the test says.
    fn commit_options(sha: &str, answers: &Answers) -> QueryOptions<String> {
        let answers = Rc::clone(answers);
        QueryOptions::new(
            crate::data::keys::commit_message(&repo(), sha),
            move |_, _| {
                let (answer, answered) = oneshot::channel();
                answers.borrow_mut().push(answer);
                async move { answered.await.unwrap() }.boxed()
            },
        )
        .stale_time(StaleTime::Infinite)
    }

    fn answer(answers: &Answers, result: Result<&str, &str>) {
        let answer = answers.borrow_mut().remove(0);
        let _ = answer.send(result.map(str::to_owned).map_err(QueryError::msg));
    }

    struct Shown {
        query: Query<String>,
    }

    #[gpui_kit::test]
    fn keeps_what_was_shown_while_another_loads(cx: &mut TestAppContext) {
        cx.update(QueryClient::init);
        let answers = Answers::default();
        let shown = cx.new(|cx| Shown {
            query: Query::new(commit_options("a", &answers), cx),
        });
        cx.run_until_parked();
        answer(&answers, Ok("First"));
        cx.run_until_parked();

        shown.update(cx, |shown, cx| {
            shown
                .query
                .set_options(commit_options("b", &answers).keep_previous_data(), cx)
        });
        cx.run_until_parked();
        shown.read_with(cx, |shown, _| {
            assert_eq!(shown.query.data().as_deref(), Some("First"));
            assert!(shown.query.is_placeholder_data());
            assert!(shown.query.is_success());
            assert!(shown.query.is_refetching());
        });
        answer(&answers, Ok("Second"));
        cx.run_until_parked();
        shown.read_with(cx, |shown, _| {
            assert_eq!(shown.query.data().as_deref(), Some("Second"));
            assert!(!shown.query.is_placeholder_data());
        });

        // Without it, nothing's shown until the other has loaded.
        shown.update(cx, |shown, cx| {
            shown.query.set_options(commit_options("c", &answers), cx)
        });
        assert!(shown.read_with(cx, |shown, _| shown.query.data()).is_none());
    }

    #[gpui_kit::test]
    fn fetches_what_isnt_fresh_once(cx: &mut TestAppContext) {
        cx.update(QueryClient::init);
        let answers = Answers::default();
        let first = cx.update(|cx| QueryClient::fetch_query(commit_options("a", &answers), cx));
        // Waits for the load under way rather than starting another.
        let second = cx.update(|cx| QueryClient::fetch_query(commit_options("a", &answers), cx));
        cx.run_until_parked();
        assert_eq!(answers.borrow().len(), 1);
        answer(&answers, Ok("Message"));
        cx.run_until_parked();
        assert_eq!(first.now_or_never().unwrap().unwrap(), "Message");
        assert_eq!(second.now_or_never().unwrap().unwrap(), "Message");

        // Fresh: what's cached.
        let cached = cx.update(|cx| QueryClient::fetch_query(commit_options("a", &answers), cx));
        cx.run_until_parked();
        assert!(answers.borrow().is_empty());
        assert_eq!(cached.now_or_never().unwrap().unwrap(), "Message");

        let failed = cx.update(|cx| QueryClient::fetch_query(commit_options("b", &answers), cx));
        cx.run_until_parked();
        answer(&answers, Err("No such commit."));
        cx.run_until_parked();
        assert_eq!(
            failed.now_or_never().unwrap().unwrap_err().message(),
            "No such commit."
        );
    }

    #[gpui_kit::test]
    fn cancels_a_load_leaving_the_data_alone(cx: &mut TestAppContext) {
        cx.update(QueryClient::init);
        let answers = Answers::default();
        let key = crate::data::keys::commit_message(&repo(), "a");
        let shown = cx.new(|cx| Shown {
            query: Query::new(commit_options("a", &answers), cx),
        });
        cx.run_until_parked();
        answer(&answers, Ok("Before"));
        cx.run_until_parked();

        drop(shown.update(cx, |shown, cx| shown.query.refetch(cx)));
        cx.update(|cx| QueryClient::cancel(&key, cx));
        assert!(cx.update(|cx| QueryClient::update_data::<String>(
            &key,
            |last| format!("{last}, changed"),
            cx
        )));
        answer(&answers, Ok("Reloaded"));
        cx.run_until_parked();
        shown.read_with(cx, |shown, _| {
            assert_eq!(shown.query.data().as_deref(), Some("Before, changed"));
            assert!(!shown.query.is_fetching());
        });
        // Nothing to change where there's nothing.
        let other = crate::data::keys::commit_message(&repo(), "b");
        assert!(!cx.update(|cx| QueryClient::update_data::<String>(&other, Clone::clone, cx)));
    }

    #[gpui_kit::test]
    fn drops_a_query_with_a_gc_time_of_its_own_sooner(cx: &mut TestAppContext) {
        cx.update(QueryClient::init);
        let answers = Answers::default();
        let options = commit_options("a", &answers).gc_time(Duration::from_secs(60));
        let key = options.key().clone();
        cx.update(|cx| QueryClient::fetch_query(options, cx).detach());
        cx.run_until_parked();
        answer(&answers, Ok("Message"));
        cx.run_until_parked();
        // Noticed unshown, then kept for a minute.
        cx.update(|cx| QueryClient::entry::<i32>(&crate::data::keys::log(&repo()), cx));
        cx.executor().advance_clock(Duration::from_secs(60));
        cx.update(|cx| QueryClient::entry::<i32>(&crate::data::keys::log(&repo()), cx));
        assert!(
            cx.update(|cx| QueryClient::get_entry::<String>(&key, cx))
                .is_none()
        );
    }

    #[gpui_kit::test]
    fn a_watched_query_doesnt_keep_an_unwatched_ones_settings(cx: &mut TestAppContext) {
        cx.update(QueryClient::init);
        let loads = Rc::new(Cell::new(0));
        // The home page's, loaded whenever it's shown.
        let home = cx.new(|cx| Probe {
            _query: Query::new(crate::data::watch::unwatched(options(&loads)), cx),
        });
        cx.run_until_parked();
        drop(home);
        cx.update(|_| {});

        // The repository's page's, of the same key: only loaded again once invalidated.
        let _repository = cx.new(|cx| Probe {
            _query: Query::new(options(&loads), cx),
        });
        cx.run_until_parked();
        cx.update(QueryClient::window_focused);
        cx.run_until_parked();
        assert_eq!(loads.get(), 1);
    }

    #[gpui_kit::test]
    fn setting_a_querys_data_makes_it_fresh(cx: &mut TestAppContext) {
        cx.update(QueryClient::init);
        let key = crate::data::keys::refs(&repo());
        cx.update(|cx| QueryClient::set_data(&key, 1, cx));
        cx.update(|cx| {
            let _ = QueryClient::invalidate(&key, cx);
        });
        cx.update(|cx| QueryClient::set_data(&key, 2, cx));
        let entry = cx.update(|cx| QueryClient::get_entry::<i32>(&key, cx).unwrap());
        assert!(!entry.read_with(cx, |entry, _| entry.is_invalidated()));
    }

    #[gpui_kit::test]
    fn drops_a_query_nothing_has_shown_for_a_while(cx: &mut TestAppContext) {
        cx.update(QueryClient::init);
        let loads = Rc::new(Cell::new(0));
        let probe = cx.new(|cx| Probe {
            _query: Query::new(options(&loads), cx),
        });
        cx.run_until_parked();
        drop(probe);
        cx.update(|_| {});
        let key = crate::data::keys::status(&repo());
        // Noticed unshown, then kept for GC_TIME.
        cx.update(|cx| QueryClient::entry::<i32>(&crate::data::keys::log(&repo()), cx));
        cx.executor().advance_clock(GC_TIME);
        cx.update(|cx| QueryClient::entry::<i32>(&crate::data::keys::log(&repo()), cx));
        assert!(
            cx.update(|cx| QueryClient::get_entry::<i32>(&key, cx))
                .is_none()
        );
    }
}
