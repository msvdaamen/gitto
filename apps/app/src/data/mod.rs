//! The data layer: what the UI loads from git and the repository list, cached, kept fresh, and
//! shown. It replaces TanStack Query (`apps/ui/src/git/queries/` and `lib/query-client.ts`) with
//! GPUI's own: entities, observation, and tasks.
//!
//! - [`QueryClient`] (a global) caches each query's [`QueryEntry`] by its [`QueryKey`]
//!   ([`keys`] are `keys.ts`'s), invalidates them by key prefix, and reloads the shown ones when
//!   the window gets focus.
//! - [`Query`] is a view's hold on one (`useQuery`): it loads it if it's stale, keeps it active,
//!   and re-renders the view when what it selects changes, and only then.
//! - [`RepositoryOperation`] runs a change to a repository (`useRepositoryOperation`), one at a
//!   time per repository, with its state in [`Operations`] (a global); [`reloading`] reloads the
//!   repository after it.
//! - [`RepositoryWatcher`] reloads a repository's data when it changes on disk
//!   (`useRepositoryWatcher`).
//!
//! # States
//!
//! A `Query` says where it is: [`Query::is_pending`] (nothing loaded), [`Query::is_loading`]
//! (loading for the first time), [`Query::is_refetching`] (loading again, with the data from before
//! on show), [`Query::is_error`] with [`Query::error`] (the data from before, if any, stays). There's
//! no Suspense: [`Query::data`] is what's loaded, or `None`, and never takes the page away.
//! (`unsuspended.ts`, which read a query's data without Suspense, has nothing to do here: every
//! read is unsuspended.)
//!
//! # Reconciling
//!
//! A load that brings data equal to what the entry had keeps what it had, and a `Query` only
//! renders its view again when its selection, status, error or loading changes: unchanged data
//! doesn't render again (TanStack's structural sharing and `reconcile`). Data too big to compare,
//! like the uncommitted files, is an [`Opaque`]: shared, equal only to itself.
//!
//! # Loading off the main thread, and cancelling
//!
//! A query's fetcher runs on the main thread, to read services, and returns a future that runs on
//! the background executor: git, SQLite and the file system never block the UI. A load is a task;
//! loading again drops it, which cancels it (as dropping a `GitApi` call's future does).
//!
//! # Writing a query
//!
//! One module per TS query file (`branch.ts` → `branch.rs`), declared below. Each query is a
//! function making its options, and one making the `Query` a view holds. With git:
//!
//! ```ignore
//! /// The branches, tags and remotes (`useRefs`).
//! pub fn refs_options(id: &RepositoryId, cx: &App) -> QueryOptions<Refs> {
//!     QueryOptions::new(keys::refs(id), loads(cx, id, |repo| repo.refs()))
//! }
//!
//! pub fn refs_query<V: 'static>(id: &RepositoryId, cx: &mut Context<V>) -> Query<Refs> {
//!     Query::new(refs_options(id, cx), cx)
//! }
//! ```
//!
//! - **Key**: from [`keys`]. Anything that changes on disk goes under `keys::repository(id)`, so
//!   the watcher reloads it, and gets [`StaleTime::Infinite`] and no reload on focus by default
//!   (see [`QueryClient::init`]); what never changes (a commit) goes outside it. A query of a
//!   repository that isn't watched (the home page's) is wrapped in [`watch::unwatched`].
//! - **Fetcher**: `call::loads(cx, id, |repo| …)` opens the repository (`GitApi::repo`) and runs
//!   one of its `GitRepo` methods, which fail with the `ApiError` the UI shows. A fetcher of its
//!   own is given the data it had, e.g. to ask git only for what changed (see `status.rs`).
//! - **Data**: `Clone + PartialEq + Send`. Make it cheap to clone and compare: an [`Opaque`] for
//!   big lists. TS's `select` is [`Query::with_select`]; each view keeps its own selection.
//! - **Options**: TS's `staleTime`, `refetchOnWindowFocus` and `enabled` are
//!   [`QueryOptions::stale_time`], [`QueryOptions::refetch_on_window_focus`] (each also `_with`,
//!   by the data) and [`QueryOptions::enabled`] / [`Query::set_enabled`].
//! - **Tests**: make the fetcher injectable (as `status::GetStatus` is) to port a test that
//!   mocked `rpc`, and use `#[gpui_kit::test]` with `cx.update(QueryClient::init)`. GPUI's test
//!   scheduler refuses wakeups from other threads, so a GPUI test can't run real git or SQLite:
//!   give it made-up sources that answer at once (`GetStatus`, `watch::WatchStreams`,
//!   `repositories::RepositorySource`, `GitApi::with` and `GitVersion::with_reader`, the system's
//!   traits), through `Services::with_*` and `crate::test_support`. Real repositories are for
//!   plain `#[test]`s with `smol::block_on`.
//!
//! A view makes its `Query`s in its constructor, and again when what they're of changes (another
//! repository, another commit): dropping the old one is all it takes. It reads them in `render`:
//!
//! ```ignore
//! struct Footer { status: Option<Query<Opaque<Uncommitted>, StatusSummary>> }
//! // …
//! if let Some(summary) = self.status.as_ref().and_then(|query| query.data()) { … }
//! ```
//!
//! What isn't kept by a key but read once (TS's `queryClient.getQueryData`) is
//! [`QueryClient::get_data`]; [`QueryClient::set_data`] and [`QueryClient::update_data`] write it.
//! TS's `fetchQuery` (loading ahead what's about to be shown, or what's read once, like a blob) is
//! [`QueryClient::fetch_query`], and `cancelQueries` is [`QueryClient::cancel`].
//!
//! # Switching what a query is of
//!
//! TS's `useQuery` follows its options as they change; a [`Query`] is told, with
//! [`Query::set_options`]: another commit's files, say. With [`QueryOptions::keep_previous_data`]
//! (TS's `placeholderData: keepPreviousData`), what was shown stays until the other has loaded,
//! as [`Query::is_placeholder_data`] says. A query that starts with what's cached elsewhere (a
//! commit from the log) has [`QueryOptions::initial_data`]; one that's big and quick to load again
//! keeps a shorter [`QueryOptions::gc_time`]. The modules give each such query's options as
//! `x_query_options`, beside `x_query`, for `set_options`.
//!
//! # Writing an operation
//!
//! TS's `useRepositoryOperation(name, repositoryId, run)` is a [`RepositoryOperation`], made once
//! (in the view's constructor, say) and run with the repository's id:
//!
//! ```ignore
//! let pull = RepositoryOperation::new("pull", |id, branch: Option<NonEmpty>, cx: &mut App| {
//!     let pulling = call(cx, &id, move |repo| repo.pull(branch));
//!     cx.spawn(async move |cx| reloading(&id, pulling, cx).await)
//! });
//! // In a click handler: runs unless it's running there; the receiver gets its result.
//! if let Some(done) = pull.run(&id, None, cx) { … }
//! // In render: whether it's running, and why it last failed.
//! let state = pull.state(&id, cx);
//! ```
//!
//! Views showing an operation observe [`Operations`] (`cx.observe_global::<Operations>`), so they
//! render again when it starts or ends. TS's `oneAtATime(operation, busy)` is
//! [`RepositoryOperation::one_at_a_time`]; `useRepositoryOperationState(name, id)` is
//! [`Operations::state`].
//!
//! TS's plain `useMutation`s (committing, staging, fetching), whose state is the view's own, are
//! [`Mutation`]s: entities a view makes and observes. Their input carries the repository's id.
//!
//! # The modules
//!
//! One per TS query file: [`branch`], [`commit`], [`conflicts`], [`diff`], [`file_diff`],
//! [`history`], [`overview`], [`progress`] (the operation under way), [`refs`], [`remote`],
//! [`staging`], [`stash`], [`status`], [`user`], [`version`], [`watch`] and [`worktree`]; and
//! `hooks/repositories.ts` as [`repositories`].

pub mod app_data;
pub mod error;
pub mod git;
pub mod keyed;
pub mod loaded;
pub mod mutation;
pub mod opaque;
pub mod repository;

pub mod branch;
pub mod changes;
pub mod commit;
pub mod conflicts;
pub mod diff;
pub mod file_diff;
pub mod history;
pub mod remote;
pub mod repositories;
pub mod staging;
pub mod status;
pub mod watch;
pub mod worktree;

pub use self::app_data::{AppChanged, AppData, AppPart};
pub use self::changes::Done;
pub use self::error::QueryError;
pub use self::loaded::Loaded;
pub use self::mutation::Mutation;
pub use self::opaque::Opaque;
pub use self::repository::{
    Changed, OperationState, Part, Repositories, Repository, Shown, observe_parts, observe_selected,
};
pub use self::watch::RepositoryWatcher;

/// Sets up the data layer's globals: the app's data, and the repositories' entities.
pub fn init(cx: &mut gpui_kit::App) {
    app_data::AppData::init(cx);
}
