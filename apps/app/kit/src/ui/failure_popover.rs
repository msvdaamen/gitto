//! Says why an action failed, under what started it. Ported from
//! `components/ui/failure-popover.tsx`.

use std::rc::Rc;

use gpui_kit::base::Popup;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Selectable, Sizable as _, ThemeStyled as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    Anchor, App, Context, ElementId, FontWeight, KeystrokeEvent, Role, SharedString, Subscription,
    TestSupportExt as _, Window, div,
};

use super::icons::{Icon, IconName};
use crate::theme::{PaletteExt as _, mixed};

type OnDismiss = Rc<dyn Fn(&mut Window, &mut App) + 'static>;

/// A button (or anything else that starts an action, and can show it's open), with a popover
/// under it that says why the action failed until dismissed: e.g. the toolbar's Pull, or a tab's
/// Remove. It's given what to say (an error's message), not the error. Dismissed by its close
/// button, Escape, or a click outside; it doesn't take focus from where the user is typing, as the
/// action can fail meanwhile: it's an alert placed by the button, not a Kit `Popover`, which
/// focuses itself.
#[derive(IntoElement)]
pub struct FailurePopover<T: Selectable + IntoElement + 'static> {
    id: ElementId,
    /// The action, e.g. "Pull".
    title: SharedString,
    /// Why it failed; `None` while it hasn't.
    error: Option<SharedString>,
    on_dismiss: Option<OnDismiss>,
    trigger: T,
}

impl<T: Selectable + IntoElement + 'static> FailurePopover<T> {
    pub fn new(
        id: impl Into<ElementId>,
        title: impl Into<SharedString>,
        error: Option<SharedString>,
        trigger: T,
    ) -> Self {
        FailurePopover {
            id: id.into(),
            title: title.into(),
            error,
            on_dismiss: None,
            trigger,
        }
    }

    pub fn on_dismiss(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_dismiss = Some(Rc::new(handler));
        self
    }
}

impl<T: Selectable + IntoElement + 'static> RenderOnce for FailurePopover<T> {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let on_dismiss = self.on_dismiss;
        let escape = window.use_keyed_state((self.id.clone(), "escape"), cx, |_, _| {
            DismissOnEscape::default()
        });
        escape.update(cx, |escape, cx| {
            let on_dismiss = self.error.is_some().then(|| on_dismiss.clone()).flatten();
            escape.set(on_dismiss, window, cx);
        });
        let open = self.error.is_some() || self.trigger.is_open();
        let popup = Popup::new(self.id, self.trigger.open(open))
            .anchor(Anchor::TopLeft)
            .offset(window.rem_size() * 0.25);
        let Some(message) = self.error else {
            return popup;
        };
        // TS's border: the coral mixed into the border, 30% of it.
        let palette = *cx.palette();
        let border = mixed(palette.coral, 0.3, palette.border);
        let dismiss = Rc::new(move |window: &mut Window, cx: &mut App| {
            if let Some(dismiss) = &on_dismiss {
                dismiss(window, cx);
            }
        });
        popup.content(
            h_flex()
                .id("failure")
                .test_support()
                // Announced, though it isn't focused.
                .role(Role::Alert)
                .items_start()
                .gap_2()
                .max_w_96()
                .popover_style(cx)
                .p_3()
                .border_1()
                .border_color(border)
                .on_mouse_down_out({
                    let dismiss = Rc::clone(&dismiss);
                    move |_, window, cx| dismiss(window, cx)
                })
                .child(
                    v_flex()
                        .min_w_0()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::BOLD)
                                .child(self.title),
                        )
                        .child(
                            div()
                                .mt_1()
                                .text_xs()
                                .text_color(palette.coral)
                                .child(message),
                        ),
                )
                .child(
                    Button::new("dismiss")
                        .ghost()
                        .xsmall()
                        .icon(Icon::new(IconName::X))
                        .accessibility_label("Dismiss")
                        .on_click(move |_, window, cx| dismiss(window, cx)),
                ),
        )
    }
}

/// Dismisses an open popover when Escape is pressed in its window and nothing else took it: it
/// isn't focused, so it isn't sent the key itself.
#[derive(Default)]
struct DismissOnEscape {
    on_dismiss: Option<OnDismiss>,
    keystrokes: Option<Subscription>,
}

impl DismissOnEscape {
    /// Listens while there's `on_dismiss`, which is while the popover's open.
    fn set(&mut self, on_dismiss: Option<OnDismiss>, window: &Window, cx: &mut Context<Self>) {
        self.on_dismiss = on_dismiss;
        if self.on_dismiss.is_none() {
            self.keystrokes = None;
        } else if self.keystrokes.is_none() {
            // Kept in the window's element state: it's always this window's popover.
            let ours = window.window_handle();
            self.keystrokes = Some(cx.observe_keystrokes(move |this, event, window, cx| {
                if window.window_handle() == ours
                    && is_unhandled_escape(event)
                    && let Some(dismiss) = this.on_dismiss.clone()
                {
                    dismiss(window, cx);
                }
            }));
        }
    }
}

/// Escape on its own, which no action took (a dialog's Cancel, say).
fn is_unhandled_escape(event: &KeystrokeEvent) -> bool {
    event.action.is_none()
        && event.keystroke.key == "escape"
        && !event.keystroke.modifiers.modified()
}

#[cfg(test)]
mod tests {
    use gpui_kit::component::input::{Input, InputState};
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{AppContext as _, Entity, Focusable as _, Render, TestAppContext, point, px};

    use super::*;
    use crate::test_support;

    /// A Pull button whose failure is shown under it, beside a commit message being typed.
    struct Host {
        message: Entity<InputState>,
        error: Option<SharedString>,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let this = cx.entity().downgrade();
            h_flex()
                .size_full()
                .items_start()
                .gap_4()
                .child(
                    FailurePopover::new(
                        "pull",
                        "Pull",
                        self.error.clone(),
                        Button::new("pull-button").label("Pull"),
                    )
                    .on_dismiss(move |_, cx| {
                        let _ = this.update(cx, |host, cx| {
                            host.error = None;
                            cx.notify();
                        });
                    }),
                )
                .child(Input::new(&self.message).w_64())
                .child(div().id("elsewhere").test_support().size_20())
        }
    }

    /// The host, with the commit message focused.
    fn typing(cx: &mut TestAppContext) -> (gpui_kit::AnyWindowHandle, Entity<Host>) {
        test_support::init(cx);
        let (window, host) = test_support::open(cx, |window, cx| Host {
            message: cx.new(|cx| InputState::new(window, cx)),
            error: None,
        });
        cx.update_window(window, |_, window, cx| {
            let message = host.read(cx).message.focus_handle(cx);
            message.focus(window, cx);
            window.input("Fix the", cx);
        })
        .unwrap();
        (window, host)
    }

    fn fail(host: &Entity<Host>, cx: &mut TestAppContext) {
        host.update(cx, |host, cx| {
            host.error = Some("Couldn't reach origin.".into());
            cx.notify();
        });
        cx.run_until_parked();
    }

    fn is_shown(window: gpui_kit::AnyWindowHandle, cx: &mut TestAppContext) -> bool {
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.try_find("failure").is_some()
        })
        .unwrap()
    }

    fn error(host: &Entity<Host>, cx: &mut TestAppContext) -> Option<SharedString> {
        host.read_with(cx, |host, _| host.error.clone())
    }

    #[gpui_kit::test]
    fn leaves_focus_where_the_user_is_typing(cx: &mut TestAppContext) {
        let (window, host) = typing(cx);
        fail(&host, cx);
        assert!(is_shown(window, cx));
        cx.update_window(window, |_, window, cx| {
            let message = host.read(cx).message.focus_handle(cx);
            assert!(message.is_focused(window));
            window.input(" parser", cx);
        })
        .unwrap();
        let typed = host.read_with(cx, |host, cx| host.message.read(cx).value());
        assert_eq!(typed, "Fix the parser");
        assert!(is_shown(window, cx));
    }

    #[gpui_kit::test]
    fn is_dismissed_by_escape_where_the_user_is_typing(cx: &mut TestAppContext) {
        let (window, host) = typing(cx);
        fail(&host, cx);
        cx.update_window(window, |_, window, cx| window.press("escape", cx))
            .unwrap();
        cx.run_until_parked();
        assert_eq!(error(&host, cx), None);
        assert!(!is_shown(window, cx));
    }

    #[gpui_kit::test]
    fn is_dismissed_by_its_close_button(cx: &mut TestAppContext) {
        let (window, host) = typing(cx);
        fail(&host, cx);
        cx.update_window(window, |_, window, cx| window.click("dismiss", cx))
            .unwrap();
        cx.run_until_parked();
        assert_eq!(error(&host, cx), None);
    }

    #[gpui_kit::test]
    fn is_dismissed_by_a_click_outside(cx: &mut TestAppContext) {
        let (window, host) = typing(cx);
        fail(&host, cx);
        cx.update_window(window, |_, window, cx| {
            window.click_at("failure", point(px(4.), px(4.)), cx)
        })
        .unwrap();
        cx.run_until_parked();
        assert!(error(&host, cx).is_some(), "a click inside keeps it");
        cx.update_window(window, |_, window, cx| window.click("elsewhere", cx))
            .unwrap();
        cx.run_until_parked();
        assert_eq!(error(&host, cx), None);
    }
}
