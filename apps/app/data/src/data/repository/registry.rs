//! Where views find a [`Repository`]: [`Repositories`], the global that opens one for each id,
//! [`Shown`], a view's of it, and the subscriptions that render a view again when it changes.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use gitto_git::input::RepositoryId;
use gpui_kit::{App, AppContext as _, Context, Entity, Global, Subscription, Task};

use super::{Changed, Field, Part, Repository, removed};
use crate::data::error::DataError;
use crate::data::git::{GitRepository, Removed, RepositoryGit};
use crate::services::Services;

/// Renders the view `cx` is of again when what `select` picks of `repository` changes, as it
/// says `part` changed, and only then: e.g. the status's summary, which stays the same as files
/// are edited.
pub fn observe_selected<V: 'static, S: PartialEq + 'static>(
    repository: &Entity<Repository>,
    part: Part,
    select: impl Fn(&Repository) -> S + 'static,
    cx: &mut Context<V>,
) -> Subscription {
    let mut last = select(repository.read(cx));
    cx.subscribe(repository, move |_, repository, Changed(changed), cx| {
        if *changed != part {
            return;
        }
        let next = select(repository.read(cx));
        if last != next {
            last = next;
            cx.notify();
        }
    })
}

/// Renders the view `cx` is of again when `repository` says one of `parts` changed.
pub fn observe_parts<V: 'static>(
    repository: &Entity<Repository>,
    parts: &'static [Part],
    cx: &mut Context<V>,
) -> Subscription {
    cx.subscribe(repository, move |_, _, Changed(changed), cx| {
        if parts.contains(changed) {
            cx.notify();
        }
    })
}

/// What a view shows of a repository: its entity, opened (see [`Repositories::open`]), with the
/// parts the view shows loaded, and rendering the view again as they change. A view keeps one, and
/// shows another repository's with [`set`](Self::set).
pub struct Shown {
    repository: Entity<Repository>,
    parts: &'static [Part],
    _changes: Subscription,
}

impl Shown {
    /// The repository `id`'s `parts`, for the view `cx` is of.
    pub fn new<V: 'static>(id: &RepositoryId, parts: &'static [Part], cx: &mut Context<V>) -> Self {
        let repository = Repositories::open(id, cx);
        Shown::of(repository, parts, cx)
    }

    fn of<V: 'static>(
        repository: Entity<Repository>,
        parts: &'static [Part],
        cx: &mut Context<V>,
    ) -> Self {
        repository.update(cx, |repository, cx| {
            for field in parts.iter().filter_map(|part| part.field()) {
                repository.show(field, cx);
            }
        });
        let changes = observe_parts(&repository, parts, cx);
        Shown {
            repository,
            parts,
            _changes: changes,
        }
    }

    pub fn entity(&self) -> &Entity<Repository> {
        &self.repository
    }

    /// The id of the repository on show.
    pub fn id<'a>(&self, cx: &'a App) -> &'a RepositoryId {
        self.repository.read(cx).id()
    }

    pub fn read<'a>(&self, cx: &'a App) -> &'a Repository {
        self.repository.read(cx)
    }

    pub fn update<R>(
        &self,
        cx: &mut App,
        update: impl FnOnce(&mut Repository, &mut Context<Repository>) -> R,
    ) -> R {
        self.repository.update(cx, update)
    }

    /// Shows the repository `id`'s, unless it's this one's; whether it was another.
    pub fn set<V: 'static>(&mut self, id: &RepositoryId, cx: &mut Context<V>) -> bool {
        if self.repository.read(cx).id() == id {
            return false;
        }
        let repository = Repositories::open(id, cx);
        *self = Shown::of(repository, self.parts, cx);
        cx.notify();
        true
    }
}

/// How many repositories that went off show keep what only their page shows: enough to switch
/// between a few tabs and see each at once, rather than every repository opened keeping its whole
/// log until Gitto quits.
const KEPT_OFF_SHOW: usize = 3;

/// Every repository opened, by id: each one's [`Repository`], kept while the app runs, so what's
/// loaded of one is there when it's shown again; of those off show long ago, only what the home
/// page shows (see [`went_off_show`](Self::went_off_show)).
#[derive(Default)]
pub struct Repositories {
    open: HashMap<RepositoryId, Entity<Repository>>,
    /// Those removed from Gitto, which aren't opened again.
    closed: HashSet<RepositoryId>,
    /// Those that went off show and keep what only their page shows, the one longest ago first.
    off_show: Vec<RepositoryId>,
}

impl Global for Repositories {}

impl Repositories {
    /// The repository `id`, opened with the app's git if it wasn't. One that was removed from
    /// Gitto isn't opened again: what's asked for it, for a view about to go with it, fails.
    pub fn open(id: &RepositoryId, cx: &mut App) -> Entity<Repository> {
        if let Some(repositories) = cx.try_global::<Repositories>() {
            if let Some(repository) = repositories.open.get(id) {
                return repository.clone();
            }
            if repositories.closed.contains(id) {
                return cx.new(|_| Repository::new(id.clone(), Rc::new(Removed)));
            }
        }
        let git = GitRepository::new(Services::global(cx).git().clone(), id.clone());
        Repositories::open_with(id, Rc::new(git), cx)
    }

    /// Opens the repository `id` with `git`, in place of one that was: for tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn insert(
        id: &RepositoryId,
        git: Rc<dyn RepositoryGit>,
        cx: &mut App,
    ) -> Entity<Repository> {
        cx.default_global::<Repositories>().closed.remove(id);
        Repositories::open_with(id, git, cx)
    }

    fn open_with(
        id: &RepositoryId,
        git: Rc<dyn RepositoryGit>,
        cx: &mut App,
    ) -> Entity<Repository> {
        let repository = cx.new(|_| Repository::new(id.clone(), git));
        cx.default_global::<Repositories>()
            .open
            .insert(id.clone(), repository.clone());
        repository
    }

    /// Runs `change` on the repository `id`, as git's part of a change a view makes: one that was
    /// removed from Gitto meanwhile isn't opened again (as [`open`](Self::open) would), and the
    /// change fails.
    pub fn update_opened<R: 'static>(
        id: &RepositoryId,
        change: impl FnOnce(&mut Repository, &mut Context<Repository>) -> Task<Result<R, DataError>>,
        cx: &mut App,
    ) -> Task<Result<R, DataError>> {
        match Repositories::get(id, cx) {
            Some(repository) => repository.update(cx, change),
            None => Task::ready(Err(removed())),
        }
    }

    /// The repository `id`, if it's been opened.
    pub fn get(id: &RepositoryId, cx: &App) -> Option<Entity<Repository>> {
        cx.try_global::<Repositories>()?.open.get(id).cloned()
    }

    /// Loads again what's shown of every repository opened: once git works again, say, for what
    /// failed for want of it.
    pub fn reload_all(cx: &mut App) {
        let open: Vec<_> = cx
            .try_global::<Repositories>()
            .map(|repositories| repositories.open.values().cloned().collect())
            .unwrap_or_default();
        for repository in open {
            repository.update(cx, |repository, cx| {
                drop(repository.reload_all(cx));
                if repository.overview.is_used() {
                    drop(repository.reload(Field::Overview, cx));
                }
                repository.reload_failed(cx);
            });
        }
    }

    /// Forgets the repository `id`, as it was removed from Gitto: it isn't opened again.
    pub fn close(id: &RepositoryId, cx: &mut App) {
        let repositories = cx.default_global::<Repositories>();
        repositories.open.remove(id);
        repositories.off_show.retain(|off| off != id);
        repositories.closed.insert(id.clone());
    }

    /// Says the repository `id` came on show (see `RepositoryWatcher`).
    pub fn came_on_show(id: &RepositoryId, cx: &mut App) {
        cx.default_global::<Repositories>()
            .off_show
            .retain(|off| off != id);
    }

    /// Says the repository `id` went off show (see `RepositoryWatcher`): it drops what's kept by a
    /// key that nothing shows, and the one that went off show longest ago, past
    /// `KEPT_OFF_SHOW`, what only its page shows (see `Repository::forget_its_page`).
    pub fn went_off_show(id: &RepositoryId, cx: &mut App) {
        let Some(repository) = Repositories::get(id, cx) else {
            return;
        };
        repository.update(cx, |repository, _| repository.went_off_show());
        let repositories = cx.global_mut::<Repositories>();
        repositories.off_show.retain(|off| off != id);
        repositories.off_show.push(id.clone());
        if repositories.off_show.len() <= KEPT_OFF_SHOW {
            return;
        }
        let oldest = repositories.off_show.remove(0);
        if let Some(repository) = Repositories::get(&oldest, cx) {
            repository.update(cx, |repository, cx| repository.forget_its_page(cx));
        }
    }
}
