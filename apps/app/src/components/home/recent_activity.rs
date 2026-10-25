//! What was last done in the repositories. Ported from `components/home/recent-activity.tsx`.

use std::rc::Rc;

use gitto_git::features::overview::schema::{Activity, ActivityKind};
use gitto_repository::Repository;
use gitto_ui_logic::git::activity::describe_activity;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme as _, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{App, ElementId, FontWeight, SharedString, TestSupportExt as _, Window, div};

use super::repository_link::repository_link;
use super::summaries::RepositorySummary;
use gitto_app_data::state::clock::relative_time;
use gitto_app_kit::theme::PaletteExt as _;
use gitto_app_kit::ui::{Card, Icon, IconName, Tone};

/// How many of the latest things done, across all repositories, are shown.
pub const SHOWN: usize = 6;

/// How a kind of thing done is marked: its icon, in its tone.
fn activity_style(kind: ActivityKind) -> (IconName, Tone) {
    match kind {
        ActivityKind::Commit => (IconName::GitCommitHorizontal, Tone::Purple),
        ActivityKind::Amend => (IconName::PencilLine, Tone::Purple),
        ActivityKind::Merge => (IconName::GitMerge, Tone::Mint),
        ActivityKind::Pull => (IconName::ArrowDownToLine, Tone::Blue),
        ActivityKind::Rebase => (IconName::GitBranch, Tone::Mint),
        ActivityKind::Checkout => (IconName::GitBranch, Tone::Blue),
        ActivityKind::Reset => (IconName::Undo2, Tone::Amber),
        ActivityKind::CherryPick => (IconName::Cherry, Tone::Coral),
        ActivityKind::Revert => (IconName::RotateCcw, Tone::Amber),
        ActivityKind::Clone => (IconName::FolderDown, Tone::Blue),
        ActivityKind::Other => (IconName::Clock, Tone::Neutral),
    }
}

/// The latest things done across all the repositories, the most recent first: [`SHOWN`] of them.
pub fn recent_entries(summaries: &[RepositorySummary]) -> Vec<(Repository, Activity)> {
    let mut entries: Vec<(&Repository, &Activity)> = summaries
        .iter()
        .flat_map(|summary| {
            summary
                .overview
                .iter()
                .flat_map(|overview| overview.activity.iter())
                .map(|activity| (&summary.repository, activity))
        })
        .collect();
    // Stable, as `toSorted`: of two done at once, the one listed first stays first.
    entries.sort_by_key(|(_, activity)| std::cmp::Reverse(activity.at));
    entries
        .into_iter()
        .take(SHOWN)
        .map(|(repository, activity)| (repository.clone(), activity.clone()))
        .collect()
}

/// What was last done in any of the repositories, as their reflogs have it. Each opens its
/// repository.
#[derive(IntoElement)]
pub struct RecentActivity {
    summaries: Rc<[RepositorySummary]>,
}

impl RecentActivity {
    pub fn new(summaries: Rc<[RepositorySummary]>) -> Self {
        RecentActivity { summaries }
    }
}

impl RenderOnce for RecentActivity {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = *cx.palette();
        let entries = recent_entries(&self.summaries);
        let card = Card::new(IconName::RotateCcwClock, "Recent activity");
        if entries.is_empty() {
            return card.child(
                div()
                    .id("no-activity")
                    .test_support()
                    .px_1()
                    .py_2()
                    .text_xs()
                    .text_color(palette.faint)
                    .child("Commits, merges and checkouts in your repositories show here."),
            );
        }
        let mut rows = Vec::with_capacity(entries.len());
        for (ix, (repository, activity)) in entries.into_iter().enumerate() {
            rows.push(activity_row(ix, repository, activity, window, cx));
        }
        card.child(v_flex().children(rows))
    }
}

/// One thing done: what, in which repository, and when; it opens the repository.
fn activity_row(
    ix: usize,
    repository: Repository,
    activity: Activity,
    window: &mut Window,
    cx: &mut App,
) -> impl IntoElement + use<> {
    let palette = *cx.palette();
    let description = describe_activity(&activity);
    let (icon, tone) = activity_style(activity.kind);
    let (background, foreground) = tone.colors(&palette);
    // Its repository, when and what. Git's reflog keeps times to the second, so two entries can
    // share all three: the row's place among the shown ones tells those apart.
    let id = ElementId::from(SharedString::from(format!(
        "activity-{}-{}-{}-{ix}",
        repository.id,
        activity.at,
        activity.kind.as_str()
    )));
    let message: SharedString = activity.message.clone().into();
    let place = match description.action {
        Some(action) => format!("{} · {action}", repository.name),
        None => repository.name.clone(),
    };
    repository_link(id, &repository.id, window, cx)
        .flex()
        .flex_row()
        .items_center()
        .aria_label(SharedString::from(description.title.clone()))
        .gap_2()
        .px_1()
        .py_2()
        .rounded(cx.theme().radius)
        .when(ix > 0, |this| {
            this.border_t_1().border_color(palette.border_soft)
        })
        .hover(|style| style.bg(palette.panel_hover))
        .focus_visible(|style| style.bg(palette.panel_hover))
        .tooltip(move |window, cx| Tooltip::new(message.clone()).build(window, cx))
        .child(
            div()
                .flex()
                .flex_none()
                .size_6()
                .items_center()
                .justify_center()
                .rounded(cx.theme().radius)
                .bg(background)
                .text_color(foreground)
                .child(Icon::new(icon).size_3p5()),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .child(
                    div()
                        .truncate()
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .text_color(palette.text)
                        .child(description.title),
                )
                .child(
                    div()
                        .truncate()
                        .text_xs()
                        .text_color(palette.faint)
                        .child(place),
                ),
        )
        .child(
            div()
                .flex_none()
                .whitespace_nowrap()
                .text_xs()
                .text_color(palette.faint)
                .child(relative_time(activity.at)),
        )
}

#[cfg(test)]
mod tests {
    use gitto_git::features::overview::schema::Overview;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{Context, IntoElement, Render, TestAppContext, Window};

    use super::*;
    use crate::test_support;
    use gitto_app_data::state::router::{Route, Router};

    const GITTO: &str = "0190a6c8-0000-7000-8000-000000000001";
    const NOTES: &str = "0190a6c8-0000-7000-8000-000000000002";

    fn activity(at: i64, kind: ActivityKind, subject: &str) -> Activity {
        Activity {
            at,
            kind,
            target: Some("main".into()),
            subject: subject.into(),
            message: format!("{} {subject}", kind.as_str()),
        }
    }

    fn summary(id: &str, name: &str, activity: Vec<Activity>) -> RepositorySummary {
        RepositorySummary {
            repository: Repository {
                id: id.into(),
                name: name.into(),
                path: format!("/src/{name}"),
            },
            status: None,
            overview: Some(Overview {
                remote: None,
                fetched_at: None,
                committed_at: None,
                activity,
            }),
            error: None,
            settled: false,
        }
    }

    #[test]
    fn shows_the_latest_across_repositories() {
        let summaries = [
            summary(
                GITTO,
                "gitto",
                (0..5)
                    .map(|n| activity(n * 10, ActivityKind::Commit, &format!("gitto {n}")))
                    .collect(),
            ),
            summary(
                NOTES,
                "notes",
                vec![
                    activity(45, ActivityKind::Checkout, "notes 45"),
                    activity(5, ActivityKind::Merge, "notes 5"),
                ],
            ),
            // Not loaded yet.
            RepositorySummary {
                overview: None,
                ..summary(GITTO, "other", Vec::new())
            },
        ];
        let subjects: Vec<String> = recent_entries(&summaries)
            .into_iter()
            .map(|(_, activity)| activity.subject)
            .collect();
        assert_eq!(
            subjects,
            [
                "notes 45", "gitto 4", "gitto 3", "gitto 2", "gitto 1", "notes 5"
            ]
        );
    }

    struct Host {
        summaries: Rc<[RepositorySummary]>,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            RecentActivity::new(self.summaries.clone())
        }
    }

    #[gpui_kit::test]
    fn says_what_shows_before_anything_is_done(cx: &mut TestAppContext) {
        let _dir = test_support::init(cx, |services| services);
        let (window, _) = test_support::open(cx, |_, _| Host {
            summaries: vec![summary(GITTO, "gitto", Vec::new())].into(),
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("no-activity").is_some());
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn opens_the_repository_something_was_done_in(cx: &mut TestAppContext) {
        let _dir = test_support::init(cx, |services| services);
        let (window, _) = test_support::open(cx, |_, _| Host {
            summaries: vec![summary(
                NOTES,
                "notes",
                vec![activity(1, ActivityKind::Commit, "Write it down")],
            )]
            .into(),
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let row = SharedString::from(format!("activity-{NOTES}-1-commit-0"));
            assert_eq!(window.find(row.clone()).label(), Some("Write it down"));
            window.click(row, cx);
        })
        .unwrap();
        cx.run_until_parked();
        assert_eq!(
            cx.update(|cx| Router::current(cx)),
            Route::Repository(gitto_git::input::RepositoryId::parse(NOTES).unwrap())
        );
    }
}
