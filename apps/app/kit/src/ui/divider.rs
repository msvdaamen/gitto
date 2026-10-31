//! A thin vertical line between groups of items in a row. Ported from
//! `components/ui/divider.tsx`.

use gpui_kit::prelude::*;
use gpui_kit::{App, Window, div, px};

use crate::theme::PaletteExt as _;

/// A thin vertical line between groups of items in a row, as tall as small text.
#[derive(IntoElement, Default)]
pub struct Divider {
    tall: bool,
}

impl Divider {
    pub fn new() -> Self {
        Divider::default()
    }

    /// As tall as a toolbar's buttons, rather than a line of text.
    pub fn tall(mut self) -> Self {
        self.tall = true;
        self
    }
}

impl RenderOnce for Divider {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        div()
            .mx_1()
            .flex_none()
            // A hairline: one device pixel, not a step of the scale.
            .w(px(1.))
            .when(self.tall, |this| this.h_6())
            .when(!self.tall, |this| this.h_3())
            .bg(cx.palette().border)
    }
}
