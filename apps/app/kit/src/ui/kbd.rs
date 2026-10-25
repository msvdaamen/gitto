//! A keyboard shortcut. Ported from `components/ui/kbd.tsx`.

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::{App, SharedString, Window, div};

use crate::theme::PaletteExt as _;

/// A keyboard shortcut, like `⌘ K`, as it's written. (GPUI Kit's `Kbd` shows a bound
/// `Keystroke`; use it for an action's binding.)
#[derive(IntoElement)]
pub struct Kbd {
    keys: SharedString,
}

impl Kbd {
    pub fn new(keys: impl Into<SharedString>) -> Self {
        Kbd { keys: keys.into() }
    }
}

impl RenderOnce for Kbd {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = *cx.palette();
        div()
            .flex_none()
            .px_1()
            .py_0p5()
            .rounded_sm()
            .border_1()
            .border_color(palette.border)
            .bg(palette.panel_raised)
            .font_family(cx.theme().mono_font_family.clone())
            .text_sm()
            .text_color(palette.faint)
            .child(self.keys)
    }
}
