//! Who a commit is by, shown while its node in the graph is hovered. Ported from
//! `components/repository/history/author-tooltip.tsx`.
//!
//! TS's `AuthorTooltipProvider` shared one tooltip between all the rows, as setting one up per row
//! cost something for every row that scrolled into view. GPUI's tooltips cost nothing until one
//! shows, and the window shows one at a time: each node has its own, which waits a moment before
//! showing (so moving across the graph doesn't flash it), and hides once the pointer leaves the
//! node, the node scrolls away from under it, or it's pressed.

use gpui_kit::component::h_flex;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::*;
use gpui_kit::{AnyView, App, FontWeight, Hsla, SharedString, TestSupportExt as _, Window, div};

use super::columns::TOOLTIP_TEXT;
use crate::components::ui::Avatar;
use crate::theme::PaletteExt as _;

/// Who a commit is by, as its tooltip shows them.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphAuthor {
    pub name: SharedString,
    pub initials: SharedString,
    /// Their avatar's colour; Gitto's purple without one.
    pub color: Option<Hsla>,
}

/// The tooltip of a node by `author`: their avatar and name. For an element's `.tooltip(…)`.
pub fn author_tooltip(author: GraphAuthor) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    move |window, cx| {
        let author = author.clone();
        Tooltip::element(move |_, cx| {
            let palette = *cx.palette();
            h_flex()
                .id("author-tooltip")
                .test_support()
                .max_w_72()
                .gap_2()
                .text_size(TOOLTIP_TEXT)
                .text_color(palette.text)
                .child(
                    Avatar::new(author.initials.clone())
                        .when_some(author.color, |avatar, color| avatar.color(color)),
                )
                .child(
                    div()
                        .truncate()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(author.name.clone()),
                )
        })
        .build(window, cx)
    }
}
