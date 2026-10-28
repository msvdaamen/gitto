//! A repository in the home page's list. Ported from `components/home/repository-card.tsx`.

use std::rc::Rc;

use gitto_git::features::status::schema::StatusSummary;
use gitto_ui_logic::git::remote::{Provider, remote_provider};
use gitto_ui_logic::git::status::{head_label, sync_label};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ElementId, FontWeight, SharedString, TestSupportExt as _, Window, div,
    linear_color_stop, linear_gradient, px,
};

use super::repository_link::repository_link;
use super::summaries::RepositorySummary;
use gitto_app_data::state::clock::relative_time;
use gitto_app_kit::theme::PaletteExt as _;
use gitto_app_kit::theme::breakpoints::SMALL;
use gitto_app_kit::ui::{Badge, Icon, IconName, Tone};

/// How a remote's host is marked: the two best known in their colors, the rest neutral.
fn provider_tone(provider: &Provider) -> Tone {
    match provider {
        Provider::GitHub => Tone::Purple,
        Provider::GitLab => Tone::Blue,
        _ => Tone::Neutral,
    }
}

/// Where a repository is hosted, by its remote's URL: [`Provider::Local`] without one.
pub fn provider_of(summary: &RepositorySummary) -> Provider {
    match summary
        .overview
        .as_ref()
        .and_then(|overview| overview.remote.as_ref())
    {
        Some(remote) => remote_provider(&remote.url),
        None => Provider::Local,
    }
}

/// A repository in the home page's list: where it's hosted, its branch, and what's changed.
/// Opens it when clicked, or on Enter. It's the `ix`th of the page's summaries, which the page's
/// sections share, so drawing it copies none of it.
#[derive(IntoElement)]
pub struct RepositoryCard {
    summaries: Rc<[RepositorySummary]>,
    ix: usize,
}

impl RepositoryCard {
    pub fn new(summaries: Rc<[RepositorySummary]>, ix: usize) -> Self {
        RepositoryCard { summaries, ix }
    }
}

impl RenderOnce for RepositoryCard {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = *cx.palette();
        let summary = &self.summaries[self.ix];
        let repository = &summary.repository;
        let id = ElementId::from(SharedString::from(format!(
            "repository-card-{}",
            repository.id
        )));
        // Below `sm` the card leaves out when it was last committed to.
        let small = window.viewport_size().width < px(SMALL);
        let group = SharedString::from(format!("repository-card-group-{}", repository.id));
        let name: SharedString = repository.name.clone().into();
        let path: SharedString = repository.path.clone().into();

        // The folder icon's tile: the primary tint fading to the blue one, ringed with a little of
        // the primary color.
        let tile = div()
            .flex()
            .flex_none()
            .size_10()
            .items_center()
            .justify_center()
            .rounded(cx.theme().radius_lg)
            .border_1()
            .border_color(palette.primary.opacity(0.18))
            .bg(linear_gradient(
                145.,
                linear_color_stop(palette.primary_soft, 0.),
                linear_color_stop(palette.blue_soft, 1.),
            ))
            .text_color(palette.primary_strong)
            .child(Icon::new(IconName::FolderGit2).size_5());

        let provider = summary.overview.as_ref().map(|overview| {
            let provider = provider_of(summary);
            let url = overview.remote.as_ref().map(|remote| remote.url.clone());
            div()
                .id("provider")
                .flex_none()
                .when_some(url, |this, url| {
                    this.tooltip(move |window, cx| Tooltip::new(url.clone()).build(window, cx))
                })
                .child(Badge::new(provider.to_string()).tone(provider_tone(&provider)))
        });

        let committed_at = summary
            .overview
            .as_ref()
            .and_then(|overview| overview.committed_at);

        let content = h_flex()
            .flex_1()
            .min_w_0()
            .gap_3()
            .child(tile)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(
                        h_flex()
                            .gap_2()
                            .min_w_0()
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_base()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(palette.text)
                                    .child(name.clone()),
                            )
                            // Once it's known whether there's a remote.
                            .children(provider),
                    )
                    .child(
                        div()
                            .id("path")
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .text_color(palette.muted)
                            .tooltip({
                                let path = path.clone();
                                move |window, cx| Tooltip::new(path.clone()).build(window, cx)
                            })
                            .child(path),
                    )
                    .child(
                        h_flex()
                            .min_h_4()
                            .gap_3()
                            .text_xs()
                            .text_color(palette.muted)
                            .children(match &summary.error {
                                Some(error) => vec![
                                    h_flex()
                                        .min_w_0()
                                        .gap_1()
                                        .text_color(palette.coral)
                                        .child(
                                            Icon::new(IconName::TriangleAlert).size_3().flex_none(),
                                        )
                                        .child(div().min_w_0().truncate().child(error.to_string()))
                                        .into_any_element(),
                                ],
                                None => summary
                                    .status
                                    .as_ref()
                                    .map(|status| repository_status(status, cx))
                                    .unwrap_or_default(),
                            }),
                    ),
            )
            .when(!small, |this| {
                this.child(
                    div()
                        .id("last-commit")
                        .test_support()
                        .flex_none()
                        .self_start()
                        .pt_1()
                        .text_xs()
                        .whitespace_nowrap()
                        .text_color(palette.faint)
                        .tooltip(|window, cx| Tooltip::new("Last commit").build(window, cx))
                        .when_some(committed_at, |this, at| this.child(relative_time(at))),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_none()
                    .w_6()
                    .justify_center()
                    .text_color(palette.faint)
                    .opacity(0.)
                    .group_hover(group.clone(), |style| style.opacity(1.))
                    .child(Icon::new(IconName::ChevronRight).size_4()),
            );

        // The border shows while hovered: the button's own hover style takes the place of its
        // border's color, so the border is a frame's around it.
        div()
            .group(group.clone())
            .w_full()
            .rounded(cx.theme().radius_lg)
            .border_1()
            .border_color(gpui_kit::transparent_black())
            .group_hover(group, |style| style.border_color(palette.border))
            .child(
                repository_link(id, &repository.id)
                    .accessibility_label(name)
                    .w_full()
                    .min_h_20()
                    .py_2p5()
                    .pl_3()
                    .pr_2p5()
                    .rounded(cx.theme().radius_lg)
                    .child(content),
            )
    }
}

/// The branch, how it compares to its upstream, and how many files changed.
fn repository_status(status: &StatusSummary, cx: &App) -> Vec<AnyElement> {
    let palette = *cx.palette();
    let item = || h_flex().flex_none().gap_1().whitespace_nowrap();
    let mut items = vec![
        item()
            .child(Icon::new(IconName::GitBranch).size_3p5())
            .child(head_label(&status.head).to_owned())
            .into_any_element(),
    ];
    if let Some(upstream) = &status.upstream
        && (status.ahead > 0 || status.behind > 0)
    {
        let compared = format!("Compared with {upstream}, as last fetched");
        items.push(
            item()
                .id("sync")
                .text_color(palette.blue)
                .tooltip(move |window, cx| Tooltip::new(compared.clone()).build(window, cx))
                .child(sync_label(status.ahead, status.behind))
                .into_any_element(),
        );
    }
    let conflicted = status.counts.conflicted;
    if conflicted > 0 {
        items.push(
            item()
                .text_color(palette.coral)
                .child(format!(
                    "{conflicted} {}",
                    if conflicted == 1 {
                        "conflict"
                    } else {
                        "conflicts"
                    }
                ))
                .into_any_element(),
        );
    }
    let files = status.counts.files;
    items.push(if files > 0 {
        item()
            .text_color(palette.amber)
            .child(format!(
                "{files} {}",
                if files == 1 { "change" } else { "changes" }
            ))
            .into_any_element()
    } else {
        item()
            .text_color(palette.mint)
            .child(Icon::new(IconName::Check).size_3())
            .child("Clean")
            .into_any_element()
    });
    items
}
