//! Says why an action failed, under what started it. Ported from
//! `components/ui/failure-popover.tsx`.

use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::{Selectable, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{Anchor, App, ElementId, FontWeight, SharedString, Window, div};

use super::icons::{Icon, IconName};
use crate::theme::PaletteExt as _;

type OnDismiss = Rc<dyn Fn(&mut Window, &mut App) + 'static>;

/// A button (or anything else that starts an action, and can show it's open), with a popover
/// under it that says why the action failed until dismissed: e.g. the toolbar's Pull, or a tab's
/// Remove. It's given what to say (an error's message), not the error. Dismissed by its close button, Escape, or a click outside; it doesn't take focus from
/// where the user is typing, as the action can fail meanwhile.
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
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let open = self.error.is_some();
        let message = self.error.unwrap_or_default();
        let title = self.title;
        let on_dismiss = self.on_dismiss;
        let dismiss = on_dismiss.clone();
        Popover::new(self.id)
            .anchor(Anchor::TopLeft)
            .open(open)
            .on_open_change(move |open, window, cx| {
                if !*open && let Some(dismiss) = &dismiss {
                    dismiss(window, cx);
                }
            })
            .trigger(self.trigger)
            .content(move |_, _, cx| {
                let palette = *cx.palette();
                let dismiss = on_dismiss.clone();
                h_flex()
                    .items_start()
                    .gap_2()
                    .max_w_96()
                    .child(
                        v_flex()
                            .min_w_0()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::BOLD)
                                    .child(title.clone()),
                            )
                            .child(
                                div()
                                    .mt_1()
                                    .text_xs()
                                    .text_color(palette.coral)
                                    .child(message.clone()),
                            ),
                    )
                    .child(
                        Button::new("dismiss")
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(IconName::X))
                            .accessibility_label("Dismiss")
                            .on_click(move |_, window, cx| {
                                if let Some(dismiss) = &dismiss {
                                    dismiss(window, cx);
                                }
                            }),
                    )
            })
    }
}
