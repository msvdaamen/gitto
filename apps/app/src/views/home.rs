//! The home page. Ported from `routes/(main)/index.tsx`.

use gitto_repository::Repository;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::v_flex;
use gpui_kit::prelude::*;
use gpui_kit::{App, Context, Entity, SharedString, Subscription, Window, div, px, rems};

use crate::components::home::recent_activity::RecentActivity;
use crate::components::home::repository_list::RepositoryList;
use crate::components::home::summaries::RepositorySummaries;
use crate::components::home::welcome_hero::WelcomeHero;
use gitto_app_data::data::repositories::{OpenRepository, Opened};
use gitto_app_data::data::{AppChanged, AppData, AppPart};
use gitto_app_data::state::clock::Clock;
use gitto_app_data::state::router::{Route, Router};
use gitto_app_kit::theme::PaletteExt as _;
use gitto_app_kit::theme::breakpoints::{LARGE, MEDIUM};

/// The widest the page's content gets (`max-w-322.5`, 1290px), in rems.
pub const CONTENT_MAX_WIDTH: f32 = 86.;
/// The activity column's width (320px), in rems.
pub const ASIDE_WIDTH: f32 = 21.33;
/// The activity column's width below [`LARGE`] (280px), in rems.
pub const ASIDE_WIDTH_NARROW: f32 = 18.67;

/// The home page: a welcome, the repositories added to Gitto, and what was last done in them.
/// Read without waiting: the page shows straight away, and fills in as they load.
pub struct HomeView {
    data: Entity<AppData>,
    summaries: RepositorySummaries,
    open_repository: Entity<OpenRepository>,
    /// The repository list's search.
    search: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl HomeView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let data = AppData::global(cx);
        data.update(cx, |data, cx| {
            data.show(AppPart::Repositories, cx);
            data.show(AppPart::UserName, cx);
        });
        let open_repository = OpenRepository::new(cx);
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Find a repository…"));
        let clock = Clock::global(cx);
        let subscriptions = vec![
            cx.subscribe(&data, |home, _, AppChanged(part), cx| match part {
                AppPart::Repositories => home.sync(cx),
                AppPart::UserName => cx.notify(),
                AppPart::Focus => home.summaries.reload_all(cx),
                AppPart::GitVersion => {}
            }),
            cx.observe(&open_repository, |_, _, cx| cx.notify()),
            cx.subscribe(&open_repository, |_, _, Opened(id), cx| {
                Router::navigate(Route::Repository(id.clone()), cx);
            }),
            // The list is filtered as the search is typed.
            cx.subscribe(&search, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
            // Times are told again as time passes ("Just now", "2 minutes ago").
            cx.observe(&clock, |_, _, cx| cx.notify()),
        ];
        let mut home = HomeView {
            data,
            summaries: RepositorySummaries::new(),
            open_repository,
            search,
            _subscriptions: subscriptions,
        };
        home.sync(cx);
        home
    }

    /// Follows the list of repositories with their summaries.
    fn sync(&mut self, cx: &mut Context<Self>) {
        let repositories = self.repositories(cx);
        self.summaries.sync(&repositories, cx);
        cx.notify();
    }
}

impl HomeView {
    fn repositories(&self, cx: &App) -> Vec<Repository> {
        let data = self.data.read(cx);
        data.repositories().data().cloned().unwrap_or_default()
    }
}

impl Render for HomeView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = *cx.palette();
        let width = window.viewport_size().width;
        // Below `lg` the activity column narrows, below `md` it goes under the repositories.
        let medium = width < px(MEDIUM);
        let aside_width = if width < px(LARGE) {
            ASIDE_WIDTH_NARROW
        } else {
            ASIDE_WIDTH
        };
        let summaries = self.summaries.summaries(cx);
        let data = self.data.read(cx);
        let loaded = data.repositories().data().is_some();
        let user_name = data.user_name().data().cloned();
        let open = self.open_repository.clone();
        let open_from_list = self.open_repository.clone();
        let dismiss = self.open_repository.clone();
        div()
            .id("home")
            .size_full()
            .overflow_y_scroll()
            .bg(palette.bg)
            .map(|this| {
                if medium {
                    this.p_5()
                } else {
                    this.px_10().pt_6().pb_10()
                }
            })
            .child(
                v_flex()
                    .mx_auto()
                    .max_w(rems(CONTENT_MAX_WIDTH))
                    .w_full()
                    .gap_6()
                    .child(WelcomeHero::new(
                        user_name.flatten().map(SharedString::from),
                        summaries.clone(),
                        self.open_repository.read(cx).error().cloned(),
                        move |_, cx| open.update(cx, |open, cx| open.open(cx)),
                        move |_, cx| dismiss.update(cx, |open, cx| open.dismiss(cx)),
                    ))
                    .child(
                        div()
                            .flex()
                            .gap_5()
                            .map(|this| {
                                if medium {
                                    this.flex_col()
                                } else {
                                    // The repositories as tall as the activity, as a grid's row.
                                    this.flex_row().items_stretch()
                                }
                            })
                            .child(div().flex_1().min_w_0().child(RepositoryList::new(
                                summaries.clone(),
                                loaded,
                                &self.search,
                                move |_, cx| open_from_list.update(cx, |open, cx| open.open(cx)),
                            )))
                            .child(
                                v_flex()
                                    .when(!medium, |this| this.w(rems(aside_width)).flex_none())
                                    .gap_3()
                                    .child(RecentActivity::new(summaries)),
                            ),
                    ),
            )
    }
}
