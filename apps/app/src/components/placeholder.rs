//! What a stub renders until its area is built: its name, and the TSX it replaces. Delete this
//! once no stub uses it.

use gpui_kit::component::v_flex;
use gpui_kit::prelude::*;
use gpui_kit::{App, FontWeight, SharedString, Window, div};

use crate::theme::PaletteExt as _;

/// A dashed box naming the area that goes here, and the TSX it's ported from.
#[derive(IntoElement)]
pub struct Placeholder {
    area: SharedString,
    source: SharedString,
    detail: Option<SharedString>,
}

impl Placeholder {
    pub fn new(area: impl Into<SharedString>, source: impl Into<SharedString>) -> Self {
        Placeholder {
            area: area.into(),
            source: source.into(),
            detail: None,
        }
    }

    /// A line of what it'll show, e.g. the repository it's of.
    pub fn detail(mut self, detail: impl Into<SharedString>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

impl RenderOnce for Placeholder {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = *cx.palette();
        v_flex()
            .size_full()
            .min_h_8()
            .items_center()
            .justify_center()
            .gap_1()
            .p_2()
            .border_1()
            .border_dashed()
            .border_color(palette.border)
            .rounded_md()
            .text_xs()
            .text_color(palette.faint)
            .overflow_hidden()
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(palette.muted)
                    .child(self.area),
            )
            .child(div().child(self.source))
            .when_some(self.detail, |this, detail| this.child(div().child(detail)))
    }
}
