//! The window: the shell around the pages. Ported from `routes/__root.tsx` (the dialogs over
//! everything) and `routes/(main)/route.tsx` (the header, the page, the footer).

use gpui_kit::component::v_flex;
use gpui_kit::prelude::*;
use gpui_kit::{Context, Entity, FocusHandle, Subscription, Window, div};

use super::home::HomeView;
use super::repository::RepositoryView;
use super::settings::SettingsView;
use crate::components::footer::Footer;
use crate::components::git_version_dialog::GitVersionDialog;
use crate::components::header::Header;
use crate::components::whats_new_dialog::WhatsNewDialog;
use crate::state::router::{Route, Router};
use crate::theme::PaletteExt as _;

/// The page on show. Pages are made when they're shown and dropped when they're left, as the
/// router mounted them, so what they load stops being kept fresh; a repository's page is kept
/// when switching to another repository.
enum Page {
    Home(Entity<HomeView>),
    Settings(Entity<SettingsView>),
    Repository(Entity<RepositoryView>),
}

/// The window's content: the header with the tabs, the page the router's on, the footer, and the
/// dialogs that can come over them (git too old, what's new).
pub struct AppShell {
    header: Entity<Header>,
    footer: Entity<Footer>,
    page: Page,
    git_version: Entity<GitVersionDialog>,
    whats_new: Entity<WhatsNewDialog>,
    /// The page's place, focused while nothing in it is: GPUI only hears keys, Tab among them,
    /// along the focused element's path, so with nothing focused (where a browser has its body)
    /// Tab wouldn't reach the first control, nor Esc what's on show.
    page_focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl AppShell {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let router = Router::global(cx);
        let page_focus = cx.focus_handle();
        window.focus(&page_focus, cx);
        let subscriptions = vec![
            // When what had the focus goes (a file's changes, once its last lines are staged, or
            // the page it was on), the closest thing around it that can take the focus does, else
            // the page's place: keys go on reaching what's on show, as Esc to leave a file.
            cx.on_focus_lost(window, |shell, window, cx| {
                let target = window
                    .focus_lost_restore_target(cx)
                    .unwrap_or_else(|| shell.page_focus.clone());
                window.focus(&target, cx);
            }),
            cx.observe_in(&router, window, |shell, _, window, cx| {
                shell.show_route(window, cx)
            }),
            // What loads on focus does: the repository list, the git version until it's
            // supported (see `AppData::window_focused`).
            cx.observe_window_activation(window, |_, window, cx| {
                if window.is_window_active() {
                    crate::data::AppData::window_focused(cx);
                }
            }),
        ];
        let route = router.read(cx).route().clone();
        let page = Self::page_for(&route, window, cx);
        AppShell {
            header: cx.new(Header::new),
            footer: cx.new(Footer::new),
            page,
            git_version: cx.new(|cx| GitVersionDialog::new(window, cx)),
            whats_new: cx.new(|cx| WhatsNewDialog::new(window, cx)),
            page_focus,
            _subscriptions: subscriptions,
        }
    }

    fn page_for(route: &Route, window: &mut Window, cx: &mut Context<Self>) -> Page {
        match route {
            Route::Home => Page::Home(cx.new(|cx| HomeView::new(window, cx))),
            Route::Settings => Page::Settings(cx.new(|cx| SettingsView::new(window, cx))),
            Route::Repository(id) => {
                let id = id.clone();
                Page::Repository(cx.new(|cx| RepositoryView::new(id, window, cx)))
            }
        }
    }

    /// Shows the router's page: the same repository page for another repository, a new page
    /// otherwise.
    fn show_route(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let route = Router::current(cx);
        match (&self.page, &route) {
            (Page::Repository(view), Route::Repository(id)) => {
                let id = id.clone();
                view.update(cx, |view, cx| view.set_repository(id, window, cx));
            }
            (Page::Home(_), Route::Home) | (Page::Settings(_), Route::Settings) => {}
            _ => {
                if matches!(self.page, Page::Repository(_)) {
                    Router::set_blocker(None, cx);
                }
                self.page = Self::page_for(&route, window, cx);
            }
        }
        cx.notify();
    }

    pub fn header(&self) -> &Entity<Header> {
        &self.header
    }

    pub fn git_version(&self) -> &Entity<GitVersionDialog> {
        &self.git_version
    }

    pub fn whats_new(&self) -> &Entity<WhatsNewDialog> {
        &self.whats_new
    }

    /// The repository's page, if one's on show.
    pub fn repository_page(&self) -> Option<&Entity<RepositoryView>> {
        match &self.page {
            Page::Repository(view) => Some(view),
            _ => None,
        }
    }
}

impl Render for AppShell {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = *cx.palette();
        let page = match &self.page {
            Page::Home(view) => view.clone().into_any_element(),
            Page::Settings(view) => view.clone().into_any_element(),
            Page::Repository(view) => view.clone().into_any_element(),
        };
        v_flex()
            .size_full()
            .bg(palette.bg)
            .text_color(palette.text)
            .child(self.header.clone())
            .child(
                div()
                    .track_focus(&self.page_focus)
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .overflow_hidden()
                    .child(page),
            )
            .child(self.footer.clone())
    }
}
