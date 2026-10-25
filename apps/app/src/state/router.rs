//! Where the window is: the home page, the settings, or a repository. TanStack Router's routes
//! (`routes/`), as GPUI state: [`Router`] is an entity, in a global, that the shell observes to
//! show the page, and anything can navigate with.
//!
//! ```ignore
//! Router::navigate(Route::Repository(id), cx);
//! ```

use std::rc::Rc;

use gitto_git::input::RepositoryId;
use gpui_kit::{App, AppContext as _, Context, Entity, Global, Task};

/// A page (TS's routes): `/`, `/settings` and `/$repoId`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum Route {
    /// The home page (`(main)/index.tsx`).
    #[default]
    Home,
    /// Gitto's settings (`(main)/settings.tsx`).
    Settings,
    /// A repository added to Gitto, by its id (`(main)/$repoId.tsx`).
    Repository(RepositoryId),
}

impl Route {
    /// The repository on show, if it's one's page (TS's `params().repoId`).
    pub fn repository_id(&self) -> Option<&RepositoryId> {
        match self {
            Route::Repository(id) => Some(id),
            _ => None,
        }
    }
}

/// What leaving the page waits for: resolves to whether to go on (TS's `useBlocker`), e.g. once a
/// file's edits are saved.
pub type Blocker = Rc<dyn Fn(&mut App) -> Task<bool>>;

/// The page on show (see the module's doc).
#[derive(Default)]
pub struct Router {
    route: Route,
    blocker: Option<Blocker>,
    /// Navigating, once the blocker says it may.
    pending: Option<Task<()>>,
}

struct GlobalRouter(Entity<Router>);

impl Global for GlobalRouter {}

impl Router {
    /// Sets up the router on `route`.
    pub fn init(route: Route, cx: &mut App) -> Entity<Router> {
        let router = cx.new(|_| Router {
            route,
            ..Router::default()
        });
        cx.set_global(GlobalRouter(router.clone()));
        router
    }

    pub fn global(cx: &App) -> Entity<Router> {
        cx.global::<GlobalRouter>().0.clone()
    }

    /// The page on show.
    pub fn current(cx: &App) -> Route {
        Router::global(cx).read(cx).route.clone()
    }

    pub fn route(&self) -> &Route {
        &self.route
    }

    /// Goes to `route`, once the page's blocker, if any, lets it.
    pub fn navigate(route: Route, cx: &mut App) {
        Router::global(cx).update(cx, |router, cx| router.go(route, cx));
    }

    /// Has leaving the page wait for `blocker`; `None` lets it go at once.
    pub fn set_blocker(blocker: Option<Blocker>, cx: &mut App) {
        Router::global(cx).update(cx, |router, _| router.blocker = blocker);
    }

    fn go(&mut self, route: Route, cx: &mut Context<Self>) {
        if route == self.route {
            return;
        }
        let Some(blocker) = self.blocker.clone() else {
            self.route = route;
            self.pending = None;
            cx.notify();
            return;
        };
        let may_leave = blocker(cx);
        self.pending = Some(cx.spawn(async move |this, cx| {
            if may_leave.await {
                let _ = this.update(cx, |router, cx| {
                    router.route = route;
                    cx.notify();
                });
            }
        }));
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::TestAppContext;

    use super::*;

    #[gpui_kit::test]
    fn goes_where_its_told_once_the_page_lets_it(cx: &mut TestAppContext) {
        cx.update(|cx| Router::init(Route::Home, cx));
        cx.update(|cx| Router::navigate(Route::Settings, cx));
        assert_eq!(cx.update(|cx| Router::current(cx)), Route::Settings);

        let blocker: Blocker = Rc::new(|cx| cx.background_spawn(async { false }));
        cx.update(|cx| Router::set_blocker(Some(blocker), cx));
        cx.update(|cx| Router::navigate(Route::Home, cx));
        cx.run_until_parked();
        assert_eq!(cx.update(|cx| Router::current(cx)), Route::Settings);

        let blocker: Blocker = Rc::new(|cx| cx.background_spawn(async { true }));
        cx.update(|cx| Router::set_blocker(Some(blocker), cx));
        cx.update(|cx| Router::navigate(Route::Home, cx));
        cx.run_until_parked();
        assert_eq!(cx.update(|cx| Router::current(cx)), Route::Home);
    }
}
