//! What the app loads that isn't a repository's: the repositories added to Gitto, the installed
//! git, and the name the user commits with. [`AppData`] holds them, as a repository's entity holds
//! its data (see `super::repository`), in a global; views subscribe to [`AppChanged`].
//!
//! Unlike a repository's, which only the watcher reloads, these load again when the window gets
//! focus ([`AppData::window_focused`]), each as it needs: the list of repositories always (it's
//! quick, and another Gitto may have changed it), the git version until it's one Gitto supports
//! (back from installing a newer one), and the user's name until it's set (in a terminal, say).
//! Views of what isn't watched, like the home page's repositories, reload what they show on
//! [`AppPart::Focus`].

use futures::FutureExt as _;
use futures::channel::oneshot;
use gitto_git::GitInstall;
use gitto_git::features::user::commands::get_user_name;
use gitto_git::features::version::check;
use gitto_repository::Repository;
use gpui_kit::{App, AppContext as _, Context, Entity, EventEmitter, Global};

use super::error::QueryError;
use super::git::GitTask;
use super::loaded::Loaded;
use crate::services::Services;

/// What of the app's data changed (see [`AppChanged`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppPart {
    /// The repositories added to Gitto.
    Repositories,
    GitVersion,
    UserName,
    /// The window got focus: what isn't watched is to be loaded again where it's shown.
    Focus,
}

/// What [`AppData`] says when what it holds of `AppPart` changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppChanged(pub AppPart);

/// The app's data (see the module's doc).
#[derive(Default)]
pub struct AppData {
    repositories: Loaded<Vec<Repository>>,
    git_version: Loaded<GitInstall>,
    user_name: Loaded<Option<String>>,
}

impl EventEmitter<AppChanged> for AppData {}

struct GlobalAppData(Entity<AppData>);

impl Global for GlobalAppData {}

impl AppData {
    /// Sets it up, with nothing loaded.
    pub fn init(cx: &mut App) {
        let data = cx.new(|_| AppData::default());
        cx.set_global(GlobalAppData(data));
    }

    pub fn global(cx: &App) -> Entity<AppData> {
        cx.global::<GlobalAppData>().0.clone()
    }

    /// The repositories added to Gitto (name and path), in the order of their tabs.
    pub fn repositories(&self) -> &Loaded<Vec<Repository>> {
        &self.repositories
    }

    /// The installed git, and whether Gitto works with it.
    pub fn git_version(&self) -> &Loaded<GitInstall> {
        &self.git_version
    }

    /// The name the user commits with, from git's global config; `None` if it isn't set.
    pub fn user_name(&self) -> &Loaded<Option<String>> {
        &self.user_name
    }

    /// Loads `part` unless it's loaded, or loading: for a view about to show it. The repository
    /// list is loaded again whenever it's shown.
    pub fn show(&mut self, part: AppPart, cx: &mut Context<Self>) {
        let (stale, loading) = match part {
            AppPart::Repositories => (true, self.repositories.is_fetching()),
            AppPart::GitVersion => (
                self.git_version.is_stale() || !self.is_supported(),
                self.git_version.is_fetching(),
            ),
            AppPart::UserName => (self.user_name.is_stale(), self.user_name.is_fetching()),
            AppPart::Focus => (false, false),
        };
        if stale && !loading {
            drop(self.reload(part, cx));
        }
    }

    fn is_supported(&self) -> bool {
        self.git_version
            .data()
            .is_some_and(|install| install.supported)
    }

    /// Loads `part` again now, in place of a load under way; a receiver told once it's done.
    pub fn reload(
        &mut self,
        part: AppPart,
        cx: &mut Context<Self>,
    ) -> Option<oneshot::Receiver<()>> {
        match part {
            AppPart::Repositories => {
                let list = Services::global(cx).repository_source().list();
                self.start(part, |data| &mut data.repositories, list, cx)
            }
            AppPart::GitVersion => {
                let git = Services::global(cx).git().clone();
                let fetch = async move { Ok(check(git.version()).await) }.boxed();
                self.start(part, |data| &mut data.git_version, fetch, cx)
            }
            AppPart::UserName => {
                let fetch = async { Ok(get_user_name().await) }.boxed();
                self.start(part, |data| &mut data.user_name, fetch, cx)
            }
            AppPart::Focus => None,
        }
    }

    /// Loads, with `fetch`, what `at` finds, in place of a load under way.
    fn start<T: PartialEq + Send + 'static>(
        &mut self,
        part: AppPart,
        at: fn(&mut Self) -> &mut Loaded<T>,
        fetch: GitTask<T>,
        cx: &mut Context<Self>,
    ) -> Option<oneshot::Receiver<()>> {
        let fetch = cx.background_spawn(fetch);
        at(self).start(cx.spawn(async move |this, cx| {
            let result: Result<T, QueryError> = fetch.await;
            let _ = this.update(cx, |data, cx| {
                at(data).settle(result);
                cx.emit(AppChanged(part));
                cx.notify();
            });
        }));
        cx.emit(AppChanged(part));
        cx.notify();
        at(self).wait()
    }

    /// Loads again what's shown of the repositories and the user's name: once git works again,
    /// say, for what failed for want of it.
    pub fn reload_shown(&mut self, cx: &mut Context<Self>) {
        if self.repositories.is_used() {
            drop(self.reload(AppPart::Repositories, cx));
        }
        if self.user_name.is_used() {
            drop(self.reload(AppPart::UserName, cx));
        }
    }

    /// Loads again what's shown that loads on focus (see the module's doc), and tells the views of
    /// what isn't watched: call when the window gets focus.
    pub fn window_focused(cx: &mut App) {
        AppData::global(cx).update(cx, |data, cx| {
            if data.repositories.is_used() {
                drop(data.reload(AppPart::Repositories, cx));
            }
            if data.git_version.is_used() && !data.is_supported() {
                drop(data.reload(AppPart::GitVersion, cx));
            }
            if matches!(data.user_name.data(), Some(None)) {
                drop(data.reload(AppPart::UserName, cx));
            }
            cx.emit(AppChanged(AppPart::Focus));
        });
    }

    /// Sets the repository list, as if it had loaded: for tests.
    pub fn set_repositories(&mut self, repositories: Vec<Repository>, cx: &mut Context<Self>) {
        self.repositories.cancel();
        self.repositories.set(repositories);
        cx.emit(AppChanged(AppPart::Repositories));
        cx.notify();
    }

    /// Sets the installed git, as if it had loaded: for tests.
    pub fn set_git_version(&mut self, install: GitInstall, cx: &mut Context<Self>) {
        self.git_version.cancel();
        self.git_version.set(install);
        cx.emit(AppChanged(AppPart::GitVersion));
        cx.notify();
    }
}
