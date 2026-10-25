//! The home page. Ported from `routes/(main)/index.tsx`.

use gitto_repository::Repository;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::v_flex;
use gpui_kit::prelude::*;
use gpui_kit::{Context, Entity, Subscription, Window, div, px, rems};

use crate::components::home::recent_activity::RecentActivity;
use crate::components::home::repository_list::RepositoryList;
use crate::components::home::summaries::RepositorySummaries;
use crate::components::home::welcome_hero::WelcomeHero;
use crate::data::Query;
use crate::data::repositories::{OpenRepository, repositories_query};
use crate::data::user::user_name_query;
use crate::state::clock::Clock;
use crate::theme::PaletteExt as _;

/// The widest the page's content gets (`max-w-322.5`, 1290px), in rems.
pub const CONTENT_MAX_WIDTH: f32 = 86.;
/// The activity column's width (320px), in rems.
pub const ASIDE_WIDTH: f32 = 21.33;
/// The activity column's width below [`LARGE`] (280px), in rems.
pub const ASIDE_WIDTH_NARROW: f32 = 18.67;

/// Window widths where the layout changes, as TS's `lg` and `md`: below `LARGE` the activity
/// column narrows, below `MEDIUM` it goes under the repositories.
const LARGE: f32 = 1024.;
const MEDIUM: f32 = 768.;

/// The home page: a welcome, the repositories added to Gitto, and what was last done in them.
/// Read without waiting: the page shows straight away, and fills in as they load.
pub struct HomeView {
    repositories: Query<Vec<Repository>>,
    summaries: RepositorySummaries,
    user_name: Query<Option<String>>,
    open_repository: Entity<OpenRepository>,
    /// The repository list's search.
    search: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl HomeView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let repositories = repositories_query(cx);
        let open_repository = OpenRepository::new(cx);
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Find a repository…"));
        let clock = Clock::global(cx);
        let subscriptions = vec![
            cx.observe(repositories.entry(), |home, _, cx| home.sync(cx)),
            cx.observe(&open_repository, |_, _, cx| cx.notify()),
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
            repositories,
            summaries: RepositorySummaries::new(),
            user_name: user_name_query(cx),
            open_repository,
            search,
            _subscriptions: subscriptions,
        };
        home.sync(cx);
        home
    }

    /// Follows the list of repositories with their summaries' queries.
    fn sync(&mut self, cx: &mut Context<Self>) {
        let repositories = self.repositories.data().unwrap_or_default();
        self.summaries.sync(&repositories, cx);
        cx.notify();
    }
}

impl Render for HomeView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = *cx.palette();
        let width = window.viewport_size().width;
        let medium = width < px(MEDIUM);
        let aside_width = if width < px(LARGE) {
            ASIDE_WIDTH_NARROW
        } else {
            ASIDE_WIDTH
        };
        let summaries = self.summaries.summaries();
        let loaded = self.repositories.data().is_some();
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
                        self.user_name.data(),
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
