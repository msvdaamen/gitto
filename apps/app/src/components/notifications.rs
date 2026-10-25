//! The bell. Ported from `components/notifications.tsx`.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{Anchor, App, Context, Entity, FontWeight, Subscription, Window, div};

use gitto_app_data::services::system::Update;
use gitto_app_data::state::update::UpdateStatus;
use gitto_app_kit::theme::PaletteExt as _;
use gitto_app_kit::ui::{Icon, IconName};

/// The bell, which opens what Gitto has to tell the user: for now, a newer version it's getting.
/// Its dot says there's something the user hasn't opened the bell to see since it last changed.
pub struct Notifications {
    update: Entity<UpdateStatus>,
    /// What the user last saw: an update, while downloading, is news again once it's ready.
    seen: Option<String>,
    _subscription: Subscription,
}

impl Notifications {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let update = UpdateStatus::global(cx);
        let subscription = cx.observe(&update, |_, _, cx| cx.notify());
        Notifications {
            update,
            seen: None,
            _subscription: subscription,
        }
    }

    fn current(&self, cx: &App) -> Option<Update> {
        self.update.read(cx).state()?.update.clone()
    }

    /// What the user sees of the update: its version, and whether it's ready.
    fn key(update: Option<&Update>) -> Option<String> {
        update.map(|update| format!("{}:{}", update.version, update.ready))
    }

    /// Whether there's news the bell hasn't been opened to see.
    pub fn is_unseen(&self, cx: &App) -> bool {
        let key = Self::key(self.current(cx).as_ref());
        key.is_some() && key != self.seen
    }
}

impl Render for Notifications {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = *cx.palette();
        let unseen = self.is_unseen(cx);
        let update = self.current(cx);
        let this = cx.entity();
        div()
            .relative()
            .flex()
            .child(
                Popover::new("notifications")
                    .anchor(Anchor::TopRight)
                    .on_open_change({
                        let key = Self::key(update.as_ref());
                        move |open, _, cx| {
                            if *open {
                                let key = key.clone();
                                this.update(cx, |this, cx| {
                                    this.seen = key;
                                    cx.notify();
                                });
                            }
                        }
                    })
                    .trigger(
                        Button::new("bell")
                            .ghost()
                            .small()
                            .icon(Icon::new(IconName::Bell))
                            .tooltip("Notifications")
                            .accessibility_label(if unseen {
                                "Notifications (new)"
                            } else {
                                "Notifications"
                            }),
                    )
                    .content(move |_, _, cx| {
                        let palette = *cx.palette();
                        v_flex()
                            .w_72()
                            .child(
                                div()
                                    .px_2()
                                    .pt_1p5()
                                    .pb_1()
                                    .text_sm()
                                    .font_weight(FontWeight::BOLD)
                                    .child("Notifications"),
                            )
                            .child(match update.clone() {
                                None => v_flex()
                                    .items_center()
                                    .gap_1p5()
                                    .px_4()
                                    .py_6()
                                    .text_color(palette.faint)
                                    .child(Icon::new(IconName::BellOff).size_4())
                                    .child(div().text_xs().child("You're all caught up"))
                                    .into_any_element(),
                                Some(update) => render_update(&update, cx).into_any_element(),
                            })
                    }),
            )
            .when(unseen, |this| {
                this.child(
                    div()
                        .absolute()
                        .top_1()
                        .right_1()
                        .size_1p5()
                        .rounded_full()
                        .bg(palette.coral)
                        .border_1()
                        .border_color(palette.header),
                )
            })
    }
}

/// The update Gitto's getting: downloading, or ready to restart into.
fn render_update(update: &Update, cx: &App) -> impl IntoElement + use<> {
    let palette = *cx.palette();
    h_flex()
        .items_start()
        .gap_2p5()
        .p_2()
        .child(
            div()
                .flex()
                .flex_none()
                .size_7()
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(palette.primary_soft)
                .text_color(palette.primary_strong)
                .child(
                    Icon::new(if update.ready {
                        IconName::RotateCw
                    } else {
                        IconName::Download
                    })
                    .size_3p5(),
                ),
        )
        .child(
            v_flex()
                .min_w_0()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(format!("Gitto {} is available", update.version)),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(palette.muted)
                        .child(if update.ready {
                            "It's downloaded, and installed when you restart Gitto."
                        } else {
                            "Downloading the update…"
                        }),
                )
                .when(update.ready, |this| {
                    this.child(
                        Button::new("restart-to-update")
                            .primary()
                            .small()
                            .label("Restart to update")
                            .on_click(|_, _, cx| UpdateStatus::install(cx)),
                    )
                }),
        )
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{AnyWindowHandle, AppContext as _, IntoElement, Render, TestAppContext};

    use super::*;
    use crate::test_support;
    use gitto_app_data::services::system::{UpdateChannel, UpdateState};

    struct Host {
        notifications: Entity<Notifications>,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().p_4().child(self.notifications.clone())
        }
    }

    fn state(update: Option<Update>) -> UpdateState {
        UpdateState {
            version: "1.2.3".into(),
            channel: UpdateChannel::Release,
            update,
        }
    }

    fn bell(window: AnyWindowHandle, cx: &mut TestAppContext) -> Option<String> {
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.find("bell").label().map(str::to_owned)
        })
        .unwrap()
    }

    #[gpui_kit::test]
    fn has_nothing_to_tell_without_an_update(cx: &mut TestAppContext) {
        let (updates, states) = test_support::TestUpdates::new();
        let _dir = test_support::init(cx, |services| test_support::with_updates(services, updates));
        let (window, _) = test_support::open(cx, |_, cx| Host {
            notifications: cx.new(Notifications::new),
        });
        states.unbounded_send(state(None)).unwrap();
        cx.run_until_parked();

        assert_eq!(bell(window, cx).as_deref(), Some("Notifications"));
        cx.update_window(window, |_, window, cx| {
            window.click("bell", cx);
            assert!(window.try_find("restart-to-update").is_none());
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn tells_of_a_new_version_until_its_seen_and_again_once_its_ready(cx: &mut TestAppContext) {
        let (updates, states) = test_support::TestUpdates::new();
        let _dir = test_support::init(cx, |services| {
            test_support::with_updates(services, updates.clone())
        });
        let (window, _) = test_support::open(cx, |_, cx| Host {
            notifications: cx.new(Notifications::new),
        });
        let downloading = Update {
            version: "1.2.4".into(),
            ready: false,
        };
        states.unbounded_send(state(Some(downloading))).unwrap();
        cx.run_until_parked();
        assert_eq!(bell(window, cx).as_deref(), Some("Notifications (new)"));

        cx.update_window(window, |_, window, cx| window.click("bell", cx))
            .unwrap();
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| window.press("escape", cx))
            .unwrap();
        assert_eq!(bell(window, cx).as_deref(), Some("Notifications"));

        let ready = Update {
            version: "1.2.4".into(),
            ready: true,
        };
        states.unbounded_send(state(Some(ready))).unwrap();
        cx.run_until_parked();
        assert_eq!(bell(window, cx).as_deref(), Some("Notifications (new)"));
        cx.update_window(window, |_, window, cx| window.click("bell", cx))
            .unwrap();
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.click("restart-to-update", cx);
        })
        .unwrap();
        assert_eq!(updates.installs.load(Ordering::SeqCst), 1);
    }
}
