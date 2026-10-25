//! The data layer: what the UI loads from git and the database, kept fresh, and the changes it
//! makes to a repository. It replaces TanStack Query (`apps/ui/src/git/queries/` and
//! `lib/query-client.ts`) with GPUI's own: entities, events, and tasks.
//!
//! - [`Repository`] is an entity per repository opened (in [`Repositories`], a global): each
//!   thing it loads is a field, a [`Loaded`] (see `repository`'s doc). Its methods change the
//!   repository (see [`changes`]): the toolbar's and the menus' run one at a time, and keep their
//!   [`OperationState`] there, so it outlives the view that started them.
//! - [`AppData`] (a global) holds what isn't a repository's: the repository list, the installed
//!   git, and the user's name. Those load again when the window gets focus.
//! - [`RepositoryWatcher`] reloads a repository's data when it changes on disk
//!   (`useRepositoryWatcher`); nothing else does.
//! - [`Mutation`] is a change whose state is the view's own (committing, staging, fetching):
//!   TanStack's plain `useMutation`.
//!
//! # Showing a repository's data
//!
//! A view asks for what it shows, and renders again when it changes, and only then. [`Shown`] does
//! both for a view's [`Part`]s:
//!
//! ```ignore
//! struct PullButton { repository: Shown }
//! // In the constructor:
//! let repository = Shown::new(&repository_id, &[Part::Status, Part::Operations], cx);
//! // In render:
//! let repository = self.repository.read(cx);
//! let behind = repository.status().data().map(|status| status.behind);
//! ```
//!
//! What's loaded by a key, like a commit's files or a file's patch, a view holds while it shows it
//! (`Repository::hold_*`, a [`keyed::Hold`]); the repository reloads what's held as it changes and
//! keeps a few of the rest. A view that shows another repository tells its `Shown` so
//! ([`Shown::set`]), and drops its holds. There's no Suspense: [`Loaded::data`] is what's loaded, or
//! `None`, and the data from before stays on show while it loads again, or if that fails.
//!
//! # Loading off the main thread, and cancelling
//!
//! Git, SQLite and the file system never block the UI: the repository asks git through
//! [`git::RepositoryGit`] (gitto-git's `GitRepo`), whose futures run on the background executor. A
//! load is a task; loading again drops it, which cancels it. Data too big to compare, like the
//! uncommitted files, is an [`Opaque`]: shared, equal only to itself.
//!
//! # Tests
//!
//! GPUI's test scheduler refuses wakeups from other threads, so a GPUI test can't run real git or
//! SQLite: a test gives a repository's entity its data (`set_*`), or a [`git::RepositoryGit`] of
//! its own that answers at once, and the app made-up sources (`watch::WatchStreams`,
//! `repositories::RepositorySource`), through `Services::with_*` and `crate::test_support`. Real
//! repositories are for plain `#[test]`s with `smol::block_on`.
//!
//! # The modules
//!
//! [`repository`] and [`changes`] are the entity; [`app_data`] is the app's data; [`watch`] the
//! watcher; [`repositories`] is `hooks/repositories.ts`. The rest are named after the TS query
//! files they come from, and hold what the entity's methods take and what views make of what it
//! loads: [`branch`], [`commit`], [`conflicts`], [`diff`], [`file_diff`], [`history`],
//! [`remote`], [`staging`], [`status`] and [`worktree`].

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
