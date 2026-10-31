//! Where the window is: the home page, the settings, or a repository. TanStack Router's routes
//! (`routes/`), as GPUI state: [`Router`] is an entity, in a global, that the shell observes to
//! show the page, and anything can navigate with.
//!
//! ```ignore
//! Router::navigate(Route::Repository(id), cx);
//! ```

use std::rc::{Rc, Weak};

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
/// file's edits are saved. The page that sets it keeps it: the router only holds on to it while
/// the page does (see [`Router::set_blocker`]).
pub type Blocker = Rc<BlockerFn>;

/// What a [`Blocker`] calls.
pub type BlockerFn = dyn Fn(&mut App) -> Task<bool>;

/// The page on show (see the module's doc).
#[derive(Default)]
pub struct Router {
    route: Route,
    blocker: Option<Weak<BlockerFn>>,
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

    /// Goes to `route`, once the page's blocker, if any, lets it. The blocker's asked with the
    /// router free, so it may read where the window is, or navigate.
    pub fn navigate(route: Route, cx: &mut App) {
        let router = Router::global(cx);
        let Some(blocker) = router.update(cx, |router, cx| router.go(&route, cx)) else {
            return;
        };
        let may_leave = blocker(cx);
        let going = cx.spawn({
            let router = router.downgrade();
            async move |cx| {
                if may_leave.await {
                    let _ = router.update(cx, |router, cx| {
                        router.route = route;
                        cx.notify();
                    });
                }
            }
        });
        router.update(cx, |router, _| router.pending = Some(going));
    }

    /// Has leaving the page wait for `blocker`, for as long as its owner, the page, keeps it:
    /// once it's dropped, with the page, the router goes on at once. The blocker set last is the
    /// one that counts, so a page that's made before the last one's dropped keeps its own.
    pub fn set_blocker(blocker: &Blocker, cx: &mut App) {
        let blocker = Rc::downgrade(blocker);
        Router::global(cx).update(cx, |router, _| router.blocker = Some(blocker));
    }

    /// Goes to `route` at once, unless the page's blocker is to be asked first: that, then.
    fn go(&mut self, route: &Route, cx: &mut Context<Self>) -> Option<Blocker> {
        // Staying, or going: a navigation still waiting on the blocker is called off.
        self.pending = None;
        if *route == self.route {
            return None;
        }
        let Some(blocker) = self.blocker.as_ref().and_then(Weak::upgrade) else {
            self.route = route.clone();
            cx.notify();
            return None;
        };
        Some(blocker)
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
        cx.update(|cx| Router::set_blocker(&blocker, cx));
        cx.update(|cx| Router::navigate(Route::Home, cx));
        cx.run_until_parked();
        assert_eq!(cx.update(|cx| Router::current(cx)), Route::Settings);

        let blocker: Blocker = Rc::new(|cx| cx.background_spawn(async { true }));
        cx.update(|cx| Router::set_blocker(&blocker, cx));
        cx.update(|cx| Router::navigate(Route::Home, cx));
        cx.run_until_parked();
        assert_eq!(cx.update(|cx| Router::current(cx)), Route::Home);
    }

    #[gpui_kit::test]
    fn stays_when_told_to_go_back_before_the_page_lets_it_leave(cx: &mut TestAppContext) {
        cx.update(|cx| Router::init(Route::Home, cx));
        let (allow, allowed) = futures::channel::oneshot::channel::<bool>();
        let allowed = std::cell::RefCell::new(Some(allowed));
        let blocker: Blocker = Rc::new(move |cx| {
            let allowed = allowed.borrow_mut().take();
            cx.background_spawn(async move {
                match allowed {
                    Some(allowed) => allowed.await.unwrap_or(false),
                    None => true,
                }
            })
        });
        cx.update(|cx| Router::set_blocker(&blocker, cx));
        cx.update(|cx| Router::navigate(Route::Settings, cx));
        cx.update(|cx| Router::navigate(Route::Home, cx));
        let _ = allow.send(true);
        cx.run_until_parked();
        assert_eq!(cx.update(|cx| Router::current(cx)), Route::Home);
    }

    #[gpui_kit::test]
    fn goes_at_once_once_the_blockers_owner_drops_it(cx: &mut TestAppContext) {
        cx.update(|cx| Router::init(Route::Home, cx));
        let blocker: Blocker = Rc::new(|cx| cx.background_spawn(async { false }));
        cx.update(|cx| Router::set_blocker(&blocker, cx));
        drop(blocker);

        cx.update(|cx| Router::navigate(Route::Settings, cx));
        assert_eq!(cx.update(|cx| Router::current(cx)), Route::Settings);
    }

    #[gpui_kit::test]
    fn lets_the_blocker_read_where_the_window_is(cx: &mut TestAppContext) {
        cx.update(|cx| Router::init(Route::Home, cx));
        let blocker: Blocker = Rc::new(|cx| {
            let leaving_home = Router::current(cx) == Route::Home;
            cx.background_spawn(async move { leaving_home })
        });
        cx.update(|cx| Router::set_blocker(&blocker, cx));
        cx.update(|cx| Router::navigate(Route::Settings, cx));
        cx.run_until_parked();
        assert_eq!(cx.update(|cx| Router::current(cx)), Route::Settings);
    }

    #[gpui_kit::test]
    fn keeps_the_blocker_set_last(cx: &mut TestAppContext) {
        cx.update(|cx| Router::init(Route::Home, cx));
        let first: Blocker = Rc::new(|cx| cx.background_spawn(async { true }));
        let last: Blocker = Rc::new(|cx| cx.background_spawn(async { false }));
        cx.update(|cx| Router::set_blocker(&first, cx));
        cx.update(|cx| Router::set_blocker(&last, cx));
        // The first page goes after the last one came.
        drop(first);

        cx.update(|cx| Router::navigate(Route::Settings, cx));
        cx.run_until_parked();
        assert_eq!(cx.update(|cx| Router::current(cx)), Route::Home);
    }
}
