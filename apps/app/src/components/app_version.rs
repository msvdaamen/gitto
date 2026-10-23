//! Which Gitto this is. Ported from `components/app-version.tsx`.

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Sizable as _, h_flex};
use gpui_kit::prelude::*;
use gpui_kit::{App, TestSupportExt as _, Window, div};

use crate::components::ui::{Divider, IconName, LinkButton};
use crate::services::system::{UpdateChannel, UpdateState};
use crate::state::update::UpdateStatus;
use crate::state::whats_new::WhatsNew;
use crate::theme::PaletteExt as _;

/// Which Gitto this is, which opens what's new in it, and the update it's getting: restarting
/// into it, once it's downloaded. Nothing until the updater says (its owner observes
/// `UpdateStatus`).
#[derive(IntoElement)]
pub struct AppVersion {
    state: Option<UpdateState>,
}

impl AppVersion {
    pub fn new(state: Option<UpdateState>) -> Self {
        AppVersion { state }
    }
}

impl RenderOnce for AppVersion {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let Some(current) = self.state else {
            return h_flex().into_any_element();
        };
        let palette = *cx.palette();
        let name = match current.channel {
            UpdateChannel::Nightly => "Gitto Nightly",
            UpdateChannel::Release => "Gitto",
        };
        h_flex()
            .gap_1()
            .when_some(current.update.clone(), |this, update| {
                let version = update.version.clone();
                this.child(if update.ready {
                    LinkButton::new(
                        "restart-to-update",
                        format!("Restart to update to {version}"),
                    )
                    .icon(IconName::RotateCw)
                    .on_click(|_, _, cx| UpdateStatus::install(cx))
                    .into_any_element()
                } else {
                    div()
                        .id("downloading-update")
                        .test_support()
                        .text_color(palette.faint)
                        .tooltip(move |window, cx| {
                            gpui_kit::component::tooltip::Tooltip::new(format!(
                                "Downloading Gitto {version}"
                            ))
                            .build(window, cx)
                        })
                        .child("Downloading update…")
                        .into_any_element()
                })
                .child(Divider::new())
            })
            .child(
                Button::new("whats-new")
                    .text()
                    .small()
                    .tooltip("What's new")
                    .accessibility_label(format!("{name} v{}", current.version))
                    .child(
                        h_flex()
                            .gap_1()
                            .child(div().text_color(palette.muted).child(name))
                            .child(
                                div()
                                    .font_family(cx.theme().mono_font_family.clone())
                                    .text_color(palette.faint)
                                    .child(format!("v{}", current.version)),
                            ),
                    )
                    .on_click(|_, _, cx| {
                        WhatsNew::global(cx).update(cx, |whats_new, cx| whats_new.show_all(cx));
                    }),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{Context, IntoElement, Render, TestAppContext, Window};

    use super::*;
    use crate::services::system::Update;
    use crate::test_support;

    /// A window showing the version, as `state` says.
    struct Host {
        state: Option<UpdateState>,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            AppVersion::new(self.state.clone())
        }
    }

    fn render_with(state: UpdateState, cx: &mut TestAppContext) -> gpui_kit::AnyWindowHandle {
        let (window, _) = test_support::open(cx, |_, _| Host { state: Some(state) });
        cx.update_window(window, |_, window, cx| window.render_frame(cx))
            .unwrap();
        window
    }

    #[gpui_kit::test]
    fn names_a_nightly(cx: &mut TestAppContext) {
        let _dir = test_support::init(cx, |services| services);
        let window = render_with(
            UpdateState {
                version: "1.2.4-nightly29853462".into(),
                channel: UpdateChannel::Nightly,
                update: None,
            },
            cx,
        );
        cx.update_window(window, |_, window, _| {
            assert_eq!(
                window.find("whats-new").label(),
                Some("Gitto Nightly v1.2.4-nightly29853462")
            );
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn says_an_updates_downloading(cx: &mut TestAppContext) {
        let _dir = test_support::init(cx, |services| services);
        let window = render_with(
            UpdateState {
                version: "1.2.3".into(),
                channel: UpdateChannel::Release,
                update: Some(Update {
                    version: "1.2.4".into(),
                    ready: false,
                }),
            },
            cx,
        );
        cx.update_window(window, |_, window, _| {
            assert!(window.try_find("downloading-update").is_some());
            assert!(window.try_find("restart-to-update").is_none());
            assert_eq!(window.find("whats-new").label(), Some("Gitto v1.2.3"));
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn restarts_into_an_update_thats_ready(cx: &mut TestAppContext) {
        let (updates, _states) = test_support::TestUpdates::new();
        let _dir = test_support::init(cx, |services| {
            test_support::with_updates(services, updates.clone())
        });
        let window = render_with(
            UpdateState {
                version: "1.2.3".into(),
                channel: UpdateChannel::Release,
                update: Some(Update {
                    version: "1.2.4".into(),
                    ready: true,
                }),
            },
            cx,
        );
        cx.update_window(window, |_, window, cx| {
            assert!(window.try_find("downloading-update").is_none());
            window.click("restart-to-update", cx);
        })
        .unwrap();
        assert_eq!(
            updates.installs.load(std::sync::atomic::Ordering::SeqCst),
            1
        );
    }

    #[gpui_kit::test]
    fn opens_whats_new_in_every_version(cx: &mut TestAppContext) {
        let _dir = test_support::init(cx, |services| services);
        let window = render_with(
            UpdateState {
                version: "1.2.3".into(),
                channel: UpdateChannel::Release,
                update: None,
            },
            cx,
        );
        cx.update_window(window, |_, window, cx| window.click("whats-new", cx))
            .unwrap();
        assert!(cx.update(|cx| WhatsNew::is_open(cx)));
    }
}
