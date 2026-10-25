//! A small colored dot, e.g. the footer's "Ready". Ported from `components/ui/status-dot.tsx`.

use gpui_kit::prelude::*;
use gpui_kit::{App, Hsla, Window, div};

/// A small dot in `color` (a palette color, e.g. `cx.palette().mint`).
#[derive(IntoElement)]
pub struct StatusDot {
    color: Hsla,
}

impl StatusDot {
    pub fn new(color: Hsla) -> Self {
        StatusDot { color }
    }
}

impl RenderOnce for StatusDot {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div().flex_none().size_1p5().rounded_full().bg(self.color)
    }
}
