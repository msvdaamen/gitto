//! A short state, count or classification. Ported from `components/ui/badge.tsx`.

use gpui_kit::prelude::*;
use gpui_kit::{App, FontWeight, SharedString, Window, div};

use super::tone::Tone;
use crate::theme::PaletteExt as _;

/// A pill with a short text in a [`Tone`], neutral by default. Keep most neutral: a tone carries a
/// meaning (see the Design Guides on badges).
#[derive(IntoElement)]
pub struct Badge {
    label: SharedString,
    tone: Tone,
}

impl Badge {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Badge {
            label: label.into(),
            tone: Tone::Neutral,
        }
    }

    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }
}

impl RenderOnce for Badge {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = *cx.palette();
        let (background, foreground) = self.tone.colors(&palette);
        div()
            .flex()
            .flex_none()
            .items_center()
            .px_1p5()
            .rounded_full()
            .border_1()
            .border_color(self.tone.border(&palette))
            .bg(background)
            .text_color(foreground)
            .text_xs()
            .line_height(gpui_kit::rems(1.1))
            .font_weight(FontWeight::BOLD)
            .child(self.label)
    }
}
