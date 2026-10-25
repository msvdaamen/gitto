//! Lines added and removed. Ported from `components/ui/line-stats.tsx`.

use gpui_kit::prelude::*;
use gpui_kit::{App, FontWeight, Window, div};

use crate::theme::PaletteExt as _;

/// Lines added and removed, like `+12 −3`.
#[derive(IntoElement)]
pub struct LineStats {
    additions: u64,
    deletions: u64,
}

impl LineStats {
    pub fn new(additions: u64, deletions: u64) -> Self {
        LineStats {
            additions,
            deletions,
        }
    }
}

impl RenderOnce for LineStats {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = *cx.palette();
        div()
            .flex()
            .items_center()
            .gap_1()
            .child(
                div()
                    .text_color(palette.mint)
                    .child(format!("+{}", self.additions)),
            )
            .child(
                div()
                    .text_color(palette.coral)
                    .font_weight(FontWeight::MEDIUM)
                    .child(format!("−{}", self.deletions)),
            )
    }
}
