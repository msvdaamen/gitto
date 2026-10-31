//! Someone's initials in a colored circle. Ported from `components/ui/avatar.tsx`.

use gpui_kit::prelude::*;
use gpui_kit::{App, FontWeight, Hsla, SharedString, Window, div};

use crate::theme::PaletteExt as _;

/// How big an avatar is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AvatarSize {
    /// In a row, by a name (23px).
    #[default]
    Small,
    /// On its own, in a header (34px).
    Medium,
}

/// Someone's initials (see `util::format::initials`) in a colored circle (see
/// `util::format::avatar_color`), Gitto's purple by default.
#[derive(IntoElement)]
pub struct Avatar {
    initials: SharedString,
    color: Option<Hsla>,
    size: AvatarSize,
}

impl Avatar {
    pub fn new(initials: impl Into<SharedString>) -> Self {
        Avatar {
            initials: initials.into(),
            color: None,
            size: AvatarSize::Small,
        }
    }

    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    pub fn size(mut self, size: AvatarSize) -> Self {
        self.size = size;
        self
    }
}

impl RenderOnce for Avatar {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = *cx.palette();
        div()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .rounded_full()
            .bg(self.color.unwrap_or(palette.avatar))
            .text_color(palette.avatar_text)
            .font_weight(FontWeight::EXTRA_BOLD)
            .map(|this| match self.size {
                AvatarSize::Small => this.size_6().text_xs(),
                AvatarSize::Medium => this.size_9().text_sm(),
            })
            .child(self.initials)
    }
}
