//! The home page. Ported from `routes/(main)/index.tsx`.

use gitto_repository::Repository;
use gpui_kit::component::input::InputState;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{Context, Entity, Subscription, Window, div, rems};

use crate::components::home::recent_activity::RecentActivity;
use crate::components::home::repository_list::RepositoryList;
use crate::components::home::summaries::RepositorySummaries;
use crate::components::home::welcome_hero::WelcomeHero;
use crate::data::Query;
use crate::data::repositories::{OpenRepository, repositories_query};
use crate::data::user::user_name_query;
use crate::theme::PaletteExt as _;

/// The widest the page's content gets (`max-w-322.5`, 1290px), in rems.
pub const CONTENT_MAX_WIDTH: f32 = 86.;
/// The activity column's width (320px).
pub const ASIDE_WIDTH: f32 = 21.33;

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
        let subscriptions = vec![
            cx.observe(repositories.entry(), |home, _, cx| home.sync(cx)),
            cx.observe(&open_repository, |_, _, cx| cx.notify()),
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
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = *cx.palette();
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
            .px_10()
            .pt_6()
            .pb_10()
            .child(
                v_flex()
                    .mx_auto()
                    .max_w(rems(CONTENT_MAX_WIDTH))
                    .w_full()
                    .gap_5()
                    .child(div().h_56().child(WelcomeHero::new(
                        self.user_name.data(),
                        summaries.clone(),
                        self.open_repository.read(cx).error().cloned(),
                        move |_, cx| open.update(cx, |open, cx| open.open(cx)),
                        move |_, cx| dismiss.update(cx, |open, cx| open.dismiss(cx)),
                    )))
                    .child(
                        h_flex()
                            .items_start()
                            .gap_4()
                            .child(div().flex_1().min_w_0().child(RepositoryList::new(
                                summaries.clone(),
                                loaded,
                                &self.search,
                                move |_, cx| open_from_list.update(cx, |open, cx| open.open(cx)),
                            )))
                            .child(
                                v_flex()
                                    .w(rems(ASIDE_WIDTH))
                                    .flex_none()
                                    .gap_3()
                                    .child(RecentActivity::new(summaries)),
                            ),
                    ),
            )
    }
}
