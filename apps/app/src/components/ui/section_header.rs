//! A panel section's title. Ported from `components/ui/section-header.tsx`.

use gpui_kit::component::h_flex;
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, FontWeight, SharedString, Window, div};

use super::badge::Badge;
use super::icons::{Icon, IconName};
use super::tone::Tone;
use crate::theme::PaletteExt as _;

/// A panel section's title with its item count, and optional actions on the right. Mint for the
/// staged changes, say.
#[derive(IntoElement)]
pub struct SectionHeader {
    icon: IconName,
    title: SharedString,
    count: Option<u64>,
    mint: bool,
    children: Vec<AnyElement>,
}

impl SectionHeader {
    pub fn new(icon: IconName, title: impl Into<SharedString>) -> Self {
        SectionHeader {
            icon,
            title: title.into(),
            count: None,
            mint: false,
            children: Vec::new(),
        }
    }

    pub fn count(mut self, count: u64) -> Self {
        self.count = Some(count);
        self
    }

    /// In mint, rather than the text's color.
    pub fn mint(mut self) -> Self {
        self.mint = true;
        self
    }
}

impl ParentElement for SectionHeader {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for SectionHeader {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = *cx.palette();
        let tone = if self.mint { Tone::Mint } else { Tone::Neutral };
        h_flex()
            .flex_none()
            .justify_between()
            .gap_2()
            .child(
                h_flex()
                    .gap_1p5()
                    .when(self.mint, |this| this.text_color(palette.mint))
                    .child(Icon::new(self.icon).size_4())
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::BOLD)
                            .text_color(palette.text)
                            .child(self.title),
                    )
                    .when_some(self.count, |this, count| {
                        this.child(Badge::new(count.to_string()).tone(tone))
                    }),
            )
            .children(self.children)
    }
}
