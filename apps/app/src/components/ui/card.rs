//! A small titled panel. Ported from `components/ui/card.tsx`.

use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, SharedString, Window, div};

use super::icons::{Icon, IconName};
use crate::theme::PaletteExt as _;

/// A small titled panel, with an optional action (like "View all") in its header.
#[derive(IntoElement)]
pub struct Card {
    icon: IconName,
    title: SharedString,
    action: Option<AnyElement>,
    children: Vec<AnyElement>,
}

impl Card {
    pub fn new(icon: IconName, title: impl Into<SharedString>) -> Self {
        Card {
            icon,
            title: title.into(),
            action: None,
            children: Vec::new(),
        }
    }

    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.action = Some(action.into_any_element());
        self
    }
}

impl ParentElement for Card {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Card {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = *cx.palette();
        v_flex()
            .rounded(cx.theme().radius_lg)
            .border_1()
            .border_color(palette.border)
            .bg(palette.panel)
            .p_3()
            .child(
                h_flex()
                    .mb_1()
                    .h_7()
                    .justify_between()
                    .child(
                        h_flex()
                            .gap_2()
                            .child(Icon::new(self.icon).size_4().text_color(palette.primary))
                            .child(div().text_sm().child(self.title)),
                    )
                    .children(self.action),
            )
            .children(self.children)
    }
}
