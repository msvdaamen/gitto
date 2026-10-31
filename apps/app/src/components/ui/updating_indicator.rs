//! Says the data on show is being reloaded. Ported from `components/ui/updating-indicator.tsx`.

use std::time::Duration;

use gpui_kit::component::h_flex;
use gpui_kit::prelude::*;
use gpui_kit::{
    Animation, AnimationExt as _, App, ElementId, FontWeight, Role, SharedString, Transformation,
    Window, div, percentage,
};

use super::icons::{Icon, IconName};
use crate::theme::PaletteExt as _;

/// A spinner saying the data on show is being reloaded; it stays on show meanwhile. Its label
/// says what's loading instead, e.g. more of it; "Updating…" by default.
#[derive(IntoElement)]
pub struct UpdatingIndicator {
    id: ElementId,
    label: Option<SharedString>,
}

impl UpdatingIndicator {
    pub fn new(id: impl Into<ElementId>) -> Self {
        UpdatingIndicator {
            id: id.into(),
            label: None,
        }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }
}

impl RenderOnce for UpdatingIndicator {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = *cx.palette();
        h_flex()
            .id(self.id)
            .role(Role::Status)
            .flex_none()
            .gap_1()
            .text_xs()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(palette.faint)
            .child(Icon::new(IconName::LoaderCircle).size_2p5().with_animation(
                "spin",
                Animation::new(Duration::from_secs(1)).repeat(),
                |icon, delta| icon.transform(Transformation::rotate(percentage(delta))),
            ))
            .child(div().child(self.label.unwrap_or_else(|| "Updating…".into())))
    }
}
