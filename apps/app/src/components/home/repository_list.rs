//! The repositories added to Gitto. Ported from `components/home/repository-list.tsx`.

use std::rc::Rc;

use gitto_git::features::status::schema::Head;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::InputState;
use gpui_kit::component::{ActiveTheme as _, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, Entity, SharedString, TestSupportExt as _, Window, div, px};

use super::repository_card::RepositoryCard;
use super::summaries::RepositorySummary;
use crate::components::ui::{Icon, IconName, TextInput};
use crate::theme::PaletteExt as _;
use crate::theme::breakpoints::SMALL;

type Handler = Rc<dyn Fn(&mut Window, &mut App) + 'static>;

/// Whether `haystack` has `needle` in it, ignoring case: `needle` is already lowercase.
fn contains_lowercase(haystack: &str, needle: &str) -> bool {
    haystack.char_indices().any(|(at, _)| {
        let mut lowered = haystack[at..].chars().flat_map(char::to_lowercase);
        needle.chars().all(|c| lowered.next() == Some(c))
    })
}

/// Where in `summaries` the repositories are whose name, path, branch or remote has `query` in
/// it, ignoring case and the spaces around it; all of them for an empty query.
pub fn filter_summaries(summaries: &[RepositorySummary], query: &str) -> Vec<usize> {
    let needle = query.trim().to_lowercase();
    summaries
        .iter()
        .enumerate()
        .filter(|(_, summary)| {
            if needle.is_empty() {
                return true;
            }
            let branch = summary
                .status
                .as_ref()
                .map_or("", |status| match &status.head {
                    Head::Detached { .. } => "",
                    Head::Branch { name, .. } | Head::Unborn { name } => name,
                });
            let remote = summary
                .overview
                .as_ref()
                .and_then(|overview| overview.remote.as_ref())
                .map_or("", |remote| remote.url.as_str());
            [
                summary.repository.name.as_str(),
                summary.repository.path.as_str(),
                branch,
                remote,
            ]
            .into_iter()
            .any(|field| contains_lowercase(field, &needle))
        })
        .map(|(ix, _)| ix)
        .collect()
}

/// The repositories added to Gitto, with a search box to filter them (by name, path, branch and
/// remote), and an "Open repository" button while there are none. The search's state is the home
/// page's (`search`), so it's kept while the page is; the page renders again as it changes.
#[derive(IntoElement)]
pub struct RepositoryList {
    summaries: Rc<[RepositorySummary]>,
    /// Whether the repositories have loaded: until then, there's no telling there are none.
    loaded: bool,
    pub(crate) search: Entity<InputState>,
    pub(crate) on_open_repository: Handler,
}

impl RepositoryList {
    pub fn new(
        summaries: Rc<[RepositorySummary]>,
        loaded: bool,
        search: &Entity<InputState>,
        on_open_repository: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        RepositoryList {
            summaries,
            loaded,
            search: search.clone(),
            on_open_repository: Rc::new(on_open_repository),
        }
    }
}

impl RenderOnce for RepositoryList {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = *cx.palette();
        let small = window.viewport_size().width < px(SMALL);
        let query = self.search.read(cx).value().trim().to_owned();
        let filtered = filter_summaries(&self.summaries, &query);
        let on_open_repository = self.on_open_repository;

        let header = div()
            .flex()
            .mb_4()
            .gap_4()
            .map(|this| {
                if small {
                    this.flex_col().items_start()
                } else {
                    this.flex_row().items_center().justify_between()
                }
            })
            .child(
                v_flex()
                    .gap_0p5()
                    .child(
                        div()
                            .text_lg()
                            .text_color(palette.text)
                            .child("Repositories"),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(palette.muted)
                            .child("The ones you've opened in Gitto"),
                    ),
            )
            .when(!self.summaries.is_empty(), |this| {
                this.child(
                    div().when(small, |this| this.w_full()).child(
                        TextInput::new(&self.search)
                            .icon(IconName::Search)
                            .compact(true),
                    ),
                )
            });

        let body = if self.summaries.is_empty() && self.loaded {
            empty(
                "no-repositories",
                IconName::FolderOpen,
                "No repositories yet. Open one to get started.".into(),
                Some(
                    Button::new("open-repository-from-list")
                        .primary()
                        .icon(Icon::new(IconName::FolderOpen))
                        .label("Open repository")
                        .on_click(move |_, window, cx| on_open_repository(window, cx))
                        .into_any_element(),
                ),
                cx,
            )
        } else if filtered.is_empty() && self.loaded {
            empty(
                "no-repositories-match",
                IconName::Search,
                format!("No repositories match “{query}”.").into(),
                None,
                cx,
            )
        } else {
            v_flex()
                .gap_1p5()
                .children(
                    filtered
                        .into_iter()
                        .map(|ix| RepositoryCard::new(self.summaries.clone(), ix)),
                )
                .into_any_element()
        };

        v_flex()
            .id("repository-list")
            .h_full()
            .rounded(cx.theme().radius_lg)
            .border_1()
            .border_color(palette.border)
            .bg(palette.panel)
            .p_5()
            .child(header)
            .child(body)
    }
}

/// The list's placeholder: an icon and what it's missing, centered, with what to do about it.
fn empty(
    id: &'static str,
    icon: IconName,
    message: SharedString,
    action: Option<AnyElement>,
    cx: &App,
) -> AnyElement {
    let palette = *cx.palette();
    v_flex()
        .id(id)
        .test_support()
        .aria_label(message.clone())
        .h_40()
        .items_center()
        .justify_center()
        .gap_2()
        .text_color(palette.faint)
        .child(Icon::new(icon).size_6())
        .child(div().text_sm().child(message))
        .children(action)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{AppContext as _, Context, IntoElement, Render, TestAppContext, Window};

    use super::*;
    use crate::components::home::test_support::{self as home, GITTO, NOTES};
    use crate::state::router::{Route, Router};
    use crate::test_support;

    /// The card of the repository with `id`.
    fn card(id: &str) -> SharedString {
        format!("repository-card-{id}").into()
    }

    fn summary(id: &str, name: &str, branch: &str, remote: Option<&str>) -> RepositorySummary {
        let summary = home::summary(id, name).branch(branch).changes(2);
        match remote {
            Some(url) => summary.remote(url),
            None => summary,
        }
        .build()
    }

    #[test]
    fn finds_repositories_by_name_path_branch_and_remote() {
        let summaries = [
            summary(GITTO, "gitto", "main", Some("git@github.com:msv/gitto.git")),
            summary(NOTES, "notes", "drafts", None),
        ];
        let names = |query: &str| -> Vec<String> {
            filter_summaries(&summaries, query)
                .into_iter()
                .map(|ix| summaries[ix].repository.name.clone())
                .collect()
        };
        assert_eq!(names(""), ["gitto", "notes"]);
        assert_eq!(names("  NOTES "), ["notes"]);
        assert_eq!(names("/src/git"), ["gitto"]);
        assert_eq!(names("drafts"), ["notes"]);
        assert_eq!(names("github.com"), ["gitto"]);
        assert!(names("nowhere").is_empty());
        // Each of them on its own, not run together.
        assert!(names("gitto /src").is_empty());
    }

    #[test]
    fn ignores_case_beyond_ascii() {
        let summaries = [summary(GITTO, "Ärger", "main", None)];
        assert_eq!(filter_summaries(&summaries, "ärg"), [0]);
        assert_eq!(filter_summaries(&summaries, "ÄRG"), [0]);
    }

    #[test]
    fn leaves_a_detached_head_out_of_the_search() {
        let mut detached = summary(GITTO, "gitto", "main", None);
        detached.status.as_mut().unwrap().head = Head::Detached {
            sha: "abc1234".into(),
        };
        assert!(filter_summaries(&[detached], "abc1234").is_empty());
    }

    /// A window showing the list of `summaries`, counting the times it asked to open one.
    struct Host {
        summaries: Rc<[RepositorySummary]>,
        loaded: bool,
        search: Entity<InputState>,
        opened: Rc<std::cell::Cell<usize>>,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let opened = self.opened.clone();
            RepositoryList::new(
                self.summaries.clone(),
                self.loaded,
                &self.search,
                move |_, _| opened.set(opened.get() + 1),
            )
        }
    }

    fn render_with(
        summaries: Vec<RepositorySummary>,
        loaded: bool,
        cx: &mut TestAppContext,
    ) -> (gpui_kit::AnyWindowHandle, gpui_kit::Entity<Host>) {
        let (window, host) = test_support::open(cx, |window, cx| {
            let search = cx.new(|cx| InputState::new(window, cx));
            cx.subscribe(
                &search,
                |_, _, _: &gpui_kit::component::input::InputEvent, cx| cx.notify(),
            )
            .detach();
            Host {
                summaries: summaries.into(),
                loaded,
                search,
                opened: Rc::default(),
            }
        });
        cx.update_window(window, |_, window, cx| window.render_frame(cx))
            .unwrap();
        (window, host)
    }

    #[gpui_kit::test]
    fn offers_to_open_a_repository_while_there_are_none(cx: &mut TestAppContext) {
        let _dir = test_support::init(cx, |services| services);
        let (window, host) = render_with(Vec::new(), true, cx);
        cx.update_window(window, |_, window, cx| {
            assert!(window.try_find("no-repositories").is_some());
            window.click("open-repository-from-list", cx);
        })
        .unwrap();
        assert_eq!(host.read_with(cx, |host, _| host.opened.get()), 1);
    }

    #[gpui_kit::test]
    fn says_nothing_until_the_repositories_have_loaded(cx: &mut TestAppContext) {
        let _dir = test_support::init(cx, |services| services);
        let (window, _) = render_with(Vec::new(), false, cx);
        cx.update_window(window, |_, window, _| {
            assert!(window.try_find("no-repositories").is_none());
            assert!(window.try_find("open-repository-from-list").is_none());
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn filters_the_cards_as_the_search_is_typed(cx: &mut TestAppContext) {
        let _dir = test_support::init(cx, |services| services);
        let (window, host) = render_with(
            vec![
                summary(GITTO, "gitto", "main", None),
                summary(NOTES, "notes", "drafts", None),
            ],
            true,
            cx,
        );
        let search = host.read_with(cx, |host, _| host.search.clone());
        cx.update_window(window, |_, window, cx| {
            assert!(window.try_find(card(GITTO)).is_some());
            assert!(window.try_find(card(NOTES)).is_some());
            search.update(cx, |search, cx| search.set_value("draft", window, cx));
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find(card(GITTO)).is_none());
            assert!(window.try_find(card(NOTES)).is_some());
            search.update(cx, |search, cx| search.set_value(" nothing  ", window, cx));
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(
                window.find("no-repositories-match").label(),
                Some("No repositories match “nothing”.")
            );
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn leaves_out_the_last_commit_below_the_small_breakpoint(cx: &mut TestAppContext) {
        let _dir = test_support::init(cx, |services| services);
        let (window, _) = render_with(vec![summary(GITTO, "gitto", "main", None)], true, cx);
        // `index.css`'s `sm` is 700px, not Tailwind's 640px.
        for (width, shown) in [(680., false), (720., true)] {
            cx.simulate_window_resize(window, gpui_kit::size(px(width), px(800.)));
            cx.update_window(window, |_, window, cx| {
                window.render_frame(cx);
                assert_eq!(
                    window.try_find("last-commit").is_some(),
                    shown,
                    "at {width}px"
                );
            })
            .unwrap();
        }
    }

    #[gpui_kit::test]
    fn opens_a_repository_from_its_card(cx: &mut TestAppContext) {
        let _dir = test_support::init(cx, |services| services);
        let (window, _) = render_with(vec![summary(GITTO, "gitto", "main", None)], true, cx);
        cx.update_window(window, |_, window, cx| {
            window.click(card(GITTO), cx);
        })
        .unwrap();
        cx.run_until_parked();
        assert_eq!(
            cx.update(|cx| Router::current(cx)),
            Route::Repository(gitto_git::input::RepositoryId::parse(GITTO).unwrap())
        );
    }

    #[gpui_kit::test]
    fn opens_a_repository_from_the_keyboard(cx: &mut TestAppContext) {
        let _dir = test_support::init(cx, |services| services);
        let (window, host) = render_with(vec![summary(NOTES, "notes", "main", None)], true, cx);
        let search = host.read_with(cx, |host, _| host.search.clone());
        cx.update_window(window, |_, window, cx| {
            // From the search box, Tab goes to the card.
            search.update(cx, |search, cx| search.focus(window, cx));
            window.press("tab", cx);
            assert_eq!(window.find(card(NOTES)).focused(), Some(true));
            window.press("enter", cx);
        })
        .unwrap();
        cx.run_until_parked();
        assert_eq!(
            cx.update(|cx| Router::current(cx)),
            Route::Repository(gitto_git::input::RepositoryId::parse(NOTES).unwrap())
        );
    }
}
