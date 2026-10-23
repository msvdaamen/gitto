//! Dialogs. Ported from `components/ui/dialog.tsx`.
//!
//! A dialog is GPUI Kit's, opened on the window: `window.open_dialog(cx, |dialog, window, cx| …)`
//! (`WindowExt`). The window's `Root` draws it over the app, dimmed behind it, traps focus in it,
//! closes it on Escape or a click outside, and gives focus back after. What TS's `DIALOG_BOX` and
//! `DialogPortal` did is the theme's (its `popover`, `overlay` and `radius_lg`).
//!
//! Gitto's dialogs start with a [`DialogHeading`]: an icon in a tone, the title, and what to know.
//! Build what it's about before opening it, and keep the builder cheap: it runs again whenever the
//! dialog layer is drawn.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, FontWeight, Rems, SharedString, Window, div};

use super::icons::{Icon, IconName};
use super::tone::Tone;
use crate::theme::PaletteExt as _;

/// How wide a short confirmation is (`max-w-[440px]`), in rems of the 15px base.
pub const CONFIRM_WIDTH: Rems = Rems(29.33);
/// How wide a dialog with a list is, like What's new (`max-w-[520px]`).
pub const LIST_WIDTH: Rems = Rems(34.67);

/// A dialog's heading: its icon in a tone, in a rounded square, then its title, and under it what
/// to know (`description`), if anything.
#[derive(IntoElement)]
pub struct DialogHeading {
    icon: IconName,
    tone: Tone,
    title: SharedString,
    description: Vec<AnyElement>,
}

impl DialogHeading {
    pub fn new(icon: IconName, tone: Tone, title: impl Into<SharedString>) -> Self {
        DialogHeading {
            icon,
            tone,
            title: title.into(),
            description: Vec::new(),
        }
    }

    /// A paragraph under the title.
    pub fn description(mut self, paragraph: impl IntoElement) -> Self {
        self.description.push(paragraph.into_any_element());
        self
    }
}

impl RenderOnce for DialogHeading {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = *cx.palette();
        let (background, foreground) = self.tone.colors(&palette);
        h_flex()
            .items_start()
            .gap_3()
            .child(
                div()
                    .flex()
                    .flex_none()
                    .size_9()
                    .items_center()
                    .justify_center()
                    .rounded_lg()
                    .bg(background)
                    .text_color(foreground)
                    .child(Icon::new(self.icon).size_5()),
            )
            .child(
                v_flex()
                    .min_w_0()
                    .flex_1()
                    .child(
                        div()
                            .text_base()
                            .font_weight(FontWeight::BOLD)
                            .text_color(palette.text)
                            .child(self.title),
                    )
                    .when(!self.description.is_empty(), |this| {
                        this.child(
                            v_flex()
                                .mt_2()
                                .gap_2()
                                .text_sm()
                                .text_color(palette.text_soft)
                                .children(self.description),
                        )
                    }),
            )
    }
}
