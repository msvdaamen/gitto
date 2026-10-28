//! The home page banner. Ported from `components/home/welcome-hero.tsx`.

use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Colorize as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    App, FontWeight, ObjectFit, SharedString, TestSupportExt as _, Window, div, img,
    linear_color_stop, linear_gradient, px, relative, rems,
};

use super::demo_action::demo_action;
use super::summaries::RepositorySummary;
use gitto_app_data::data::DataError;
use gitto_app_data::state::clock::relative_time;
use gitto_app_kit::theme::breakpoints::MEDIUM;
use gitto_app_kit::theme::{AppTheme, PaletteExt as _, base_px};
use gitto_app_kit::ui::{FailurePopover, Icon, IconName};

type Handler = Rc<dyn Fn(&mut Window, &mut App) + 'static>;

/// The home page banner: a greeting, a way to open a repository, and whether all are in sync.
#[derive(IntoElement)]
pub struct WelcomeHero {
    /// The name the user commits with: `None` if they haven't set one, or until it's loaded.
    pub(crate) user_name: Option<SharedString>,
    pub(crate) summaries: Rc<[RepositorySummary]>,
    /// Why the last repository couldn't be opened, under the button, until it's dismissed.
    pub(crate) open_error: Option<DataError>,
    pub(crate) on_open_repository: Handler,
    pub(crate) on_dismiss_open_error: Handler,
}

impl WelcomeHero {
    pub fn new(
        user_name: Option<SharedString>,
        summaries: Rc<[RepositorySummary]>,
        open_error: Option<DataError>,
        on_open_repository: impl Fn(&mut Window, &mut App) + 'static,
        on_dismiss_open_error: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        WelcomeHero {
            user_name,
            summaries,
            open_error,
            on_open_repository: Rc::new(on_open_repository),
            on_dismiss_open_error: Rc::new(on_dismiss_open_error),
        }
    }
}

/// The first of their names, the way people are greeted; `None` without a name.
pub fn first_name(user_name: Option<&str>) -> Option<&str> {
    user_name?.split_whitespace().next()
}

/// "Welcome back, Ada.", or without a name to greet, "Welcome back.".
pub fn greeting(user_name: Option<&str>) -> String {
    match first_name(user_name) {
        Some(name) => format!("Welcome back, {name}."),
        None => "Welcome back.".to_owned(),
    }
}

impl RenderOnce for WelcomeHero {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = *cx.palette();
        // Below `md` the sync status is left out, and the mascot moves to the corner.
        let narrow = window.viewport_size().width < px(MEDIUM);
        let greeting: SharedString = greeting(self.user_name.as_deref()).into();
        let lead = if self.summaries.is_empty() {
            "Open a git repository to get started, or shape something new."
        } else {
            "Pick up where you left off, or shape something new."
        };
        let on_open_repository = self.on_open_repository.clone();
        let on_dismiss_open_error = self.on_dismiss_open_error.clone();

        // `color-mix(in srgb, var(--primary) 30%, var(--border))`, and the panel with a tenth of
        // the primary color fading to the panel.
        let border = palette.primary.mix_oklab(palette.border, 0.3);
        let background = linear_gradient(
            122.,
            linear_color_stop(palette.panel.mix_oklab(palette.primary, 0.9), 0.),
            linear_color_stop(palette.panel, 1.),
        );

        div()
            .id("welcome-hero")
            .relative()
            .flex()
            .items_center()
            .min_h_56()
            .w_full()
            .overflow_hidden()
            .rounded(cx.theme().radius_lg)
            .border_1()
            .border_color(border)
            .bg(background)
            .shadow_md()
            .map(|this| {
                if narrow {
                    this.p_6()
                } else {
                    this.px_10().py_8()
                }
            })
            .child(HeroDecoration { narrow })
            .child(
                v_flex()
                    .relative()
                    .child(
                        h_flex()
                            .mb_3()
                            .gap_2()
                            .text_sm()
                            .font_weight(FontWeight::BOLD)
                            .text_color(palette.primary_strong)
                            .child(
                                div()
                                    .flex()
                                    .size_6()
                                    .items_center()
                                    .justify_center()
                                    .rounded(cx.theme().radius)
                                    .bg(palette.primary_soft)
                                    .child(Icon::new(IconName::Sparkles).size_3p5()),
                            )
                            .child("YOUR WORKSPACE"),
                    )
                    .child(
                        div()
                            .id("greeting")
                            .test_support()
                            .aria_label(greeting.clone())
                            .text_3xl()
                            .line_height(relative(1.1))
                            .text_color(palette.text)
                            .child(greeting),
                    )
                    .child(
                        div()
                            .mt_2()
                            .mb_5()
                            .text_sm()
                            .text_color(palette.muted)
                            .child(lead),
                    )
                    .child(
                        h_flex()
                            .flex_wrap()
                            .gap_2()
                            .child(
                                FailurePopover::new(
                                    "open-repository-failure",
                                    "Open repository",
                                    self.open_error.map(|error| error.to_string().into()),
                                    Button::new("open-repository")
                                        .primary()
                                        .icon(Icon::new(IconName::Folder))
                                        .label("Open repository")
                                        .on_click(move |_, window, cx| {
                                            on_open_repository(window, cx)
                                        }),
                                )
                                .on_dismiss(move |window, cx| on_dismiss_open_error(window, cx)),
                            )
                            .child(
                                Button::new("clone-repository")
                                    .icon(Icon::new(IconName::Copy))
                                    .label("Clone from URL")
                                    .on_click(demo_action(
                                        "Clone repository is ready for Git integration.",
                                    )),
                            )
                            .child(
                                Button::new("new-repository")
                                    .ghost()
                                    .icon(Icon::new(IconName::Plus))
                                    .label("New repository")
                                    .on_click(demo_action(
                                        "Initialize repository is a demo action.",
                                    )),
                            ),
                    ),
            )
            .when(!narrow, |this| {
                this.when_some(sync_state(&self.summaries), |this, sync| {
                    this.child(SyncStatus { sync })
                })
            })
    }
}

/// The big mascot peeking in from the top, and a blue blob in the corner: art, behind the text.
#[derive(IntoElement)]
struct HeroDecoration {
    narrow: bool,
}

impl RenderOnce for HeroDecoration {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = *cx.palette();
        div()
            .absolute()
            .inset_0()
            // Turned, smiling, and in the theme's primary color: the SVG has it all drawn in, its
            // box the turned blob's, so it's placed that much further out.
            .child(
                img(match cx.app_theme() {
                    AppTheme::Dark => "brand/hero-mascot-dark.svg",
                    AppTheme::Light => "brand/hero-mascot-light.svg",
                })
                .object_fit(ObjectFit::Contain)
                .absolute()
                .top(base_px(-60.05))
                .map(|this| {
                    if self.narrow {
                        this.right(base_px(-3.85))
                    } else {
                        this.right(base_px(96.15))
                    }
                })
                .w(base_px(207.7))
                .h(base_px(186.1))
                .opacity(0.3),
            )
            .child(
                div()
                    .absolute()
                    .right(rems(-1.9))
                    .bottom(rems(-2.5))
                    .w(rems(7.3))
                    .h(rems(6.3))
                    .rounded_full()
                    .bg(palette.blue)
                    .opacity(0.18),
            )
    }
}

/// How the repositories compare to their upstreams, as last fetched.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SyncState {
    /// How many repositories have commits to pull.
    pub behind: usize,
    /// How many have commits to push.
    pub ahead: usize,
    /// When any was last fetched, in milliseconds since the epoch; `None` if none ever was.
    pub fetched_at: Option<i64>,
}

impl SyncState {
    pub fn is_synced(&self) -> bool {
        self.behind == 0 && self.ahead == 0
    }
}

/// How the repositories compare to their upstreams: known once all their statuses have loaded,
/// and only if any has an upstream.
pub fn sync_state(summaries: &[RepositorySummary]) -> Option<SyncState> {
    if summaries.iter().any(|summary| !summary.settled) {
        return None;
    }
    let tracked: Vec<_> = summaries
        .iter()
        .filter_map(|summary| summary.status.as_ref())
        .filter(|status| status.upstream.is_some())
        .collect();
    if tracked.is_empty() {
        return None;
    }
    let fetched_at = summaries
        .iter()
        .filter_map(|summary| summary.overview.as_ref()?.fetched_at)
        .fold(0, i64::max);
    Some(SyncState {
        behind: tracked.iter().filter(|status| status.behind > 0).count(),
        ahead: tracked.iter().filter(|status| status.ahead > 0).count(),
        fetched_at: (fetched_at > 0).then_some(fetched_at),
    })
}

/// How many repositories have commits to pull and to push, e.g. "2 repositories to pull, 1 to
/// push".
pub fn out_of_sync(behind: usize, ahead: usize) -> String {
    let counts = [(behind, "to pull"), (ahead, "to push")];
    let mut counted = counts.into_iter().filter(|(count, _)| *count > 0);
    let Some((count, what)) = counted.next() else {
        return String::new();
    };
    let noun = if count == 1 {
        "repository"
    } else {
        "repositories"
    };
    std::iter::once(format!("{count} {noun} {what}"))
        .chain(counted.map(|(others, which)| format!("{others} {which}")))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Whether the repositories are in sync with their upstreams, in the banner's corner.
#[derive(IntoElement)]
struct SyncStatus {
    sync: SyncState,
}

impl RenderOnce for SyncStatus {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = *cx.palette();
        let synced = self.sync.is_synced();
        let (icon, background, foreground) = if synced {
            (IconName::Check, palette.mint_soft, palette.mint)
        } else {
            (IconName::ArrowDownUp, palette.amber_soft, palette.amber)
        };
        let title = if synced {
            "Everything is in sync".to_owned()
        } else {
            out_of_sync(self.sync.behind, self.sync.ahead)
        };
        let fetched = match self.sync.fetched_at {
            Some(at) => format!("Last fetched {}", relative_time(at).to_lowercase()),
            None => "Not fetched yet".to_owned(),
        };
        h_flex()
            .id("sync-status")
            .test_support()
            .aria_label(SharedString::from(title.clone()))
            .absolute()
            .right_9()
            .bottom_6()
            .gap_2p5()
            .px_3()
            .py_2()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(palette.border)
            .bg(palette.bg.opacity(0.82))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .size_7()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(background)
                    .text_color(foreground)
                    .child(Icon::new(icon).size_4()),
            )
            .child(
                v_flex()
                    .gap_0p5()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::BOLD)
                            .text_color(palette.text)
                            .child(title),
                    )
                    .child(div().text_xs().text_color(palette.muted).child(fetched)),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::home::test_support as home;

    fn summary(upstream: Option<&str>, ahead: u64, behind: u64) -> RepositorySummary {
        home::summary(home::GITTO, "gitto")
            .upstream(upstream, ahead, behind)
            .build()
    }

    #[test]
    fn greets_by_first_name() {
        assert_eq!(greeting(Some("  Ada  Lovelace ")), "Welcome back, Ada.");
        assert_eq!(greeting(Some("   ")), "Welcome back.");
        assert_eq!(greeting(None), "Welcome back.");
    }

    #[test]
    fn says_how_many_repositories_are_out_of_sync() {
        assert_eq!(out_of_sync(2, 1), "2 repositories to pull, 1 to push");
        assert_eq!(out_of_sync(1, 0), "1 repository to pull");
        assert_eq!(out_of_sync(0, 3), "3 repositories to push");
        assert_eq!(out_of_sync(0, 0), "");
    }

    #[test]
    fn waits_for_every_status_before_telling_the_sync() {
        let loading = home::summary(home::NOTES, "notes")
            .upstream(Some("origin/main"), 0, 1)
            .loading()
            .build();
        assert_eq!(
            sync_state(&[summary(Some("origin/main"), 0, 0), loading]),
            None
        );
    }

    #[test]
    fn tells_the_sync_only_with_an_upstream() {
        assert_eq!(sync_state(&[summary(None, 0, 0)]), None);
        let fetched = home::summary(home::GITTO, "gitto")
            .upstream(Some("origin/main"), 1, 2)
            .fetched_at(1_000)
            .build();
        assert_eq!(
            sync_state(&[
                fetched,
                summary(Some("origin/dev"), 0, 0),
                summary(None, 4, 0)
            ]),
            Some(SyncState {
                behind: 1,
                ahead: 1,
                fetched_at: Some(1_000),
            })
        );
        assert_eq!(
            sync_state(&[summary(Some("origin/main"), 0, 0)]),
            Some(SyncState {
                behind: 0,
                ahead: 0,
                fetched_at: None,
            })
        );
    }
}
