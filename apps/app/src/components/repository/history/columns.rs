//! The history table's geometry and type sizes. Ported from `components/repository/history/columns.ts`.
//!
//! The TS laid the rows out on a grid, `minmax(105px,.8fr) var(--graph-width) minmax(240px,2.2fr)
//! 100px`: here each row is a flex row whose cells take the same shares ([`Column`]). Sizes are in
//! rems of the 15px base, so the table zooms with the rest of the app; the graph's own geometry is
//! `history_graph`'s.

use std::sync::Arc;

use gpui_kit::prelude::*;
use gpui_kit::{Div, ElementId, Hsla, Rems, SharedString, rems};

/// Height of a history row, including its 1px bottom border (38px).
pub const ROW_HEIGHT: Rems = Rems(38. / 15.);

/// Height of the column headings (31px).
pub const HEADER_HEIGHT: Rems = Rems(31. / 15.);

/// The space either side of a cell's content (9px).
pub const CELL_PADDING: Rems = Rems(9. / 15.);

/// The type sizes the rows use: a label's (10.5px), a date's or a footnote's (11.5px), a commit
/// message's (13px), and a tooltip's (12px).
pub const LABEL_TEXT: Rems = Rems(10.5 / 15.);
pub const SMALL_TEXT: Rems = Rems(11.5 / 15.);
pub const MESSAGE_TEXT: Rems = Rems(13. / 15.);
pub const TOOLTIP_TEXT: Rems = Rems(12. / 15.);

/// The line heights of a label's text (14px), and of a count's in the uncommitted changes' row
/// (13px): the TS's, as the body's line height gave them.
pub const LABEL_LINE: Rems = Rems(14. / 15.);
pub const COUNT_LINE: Rems = Rems(13. / 15.);

/// The size of the icons in labels and badges (9px).
pub const LABEL_ICON: Rems = Rems(9. / 15.);

/// The table's columns: branch / tag, graph, message and date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Column {
    /// `minmax(105px, .8fr)`.
    Labels,
    /// The graph's lanes (see `history_graph::graph_width`).
    Graph,
    /// `minmax(240px, 2.2fr)`.
    Message,
    /// `100px`.
    Date,
}

/// The table's least width, less the graph's: the other columns' (445px).
pub const MIN_WIDTH: Rems = Rems(445. / 15.);

/// The date column's width (100px).
const DATE_WIDTH: Rems = Rems(100. / 15.);

impl Column {
    /// Sizes `cell` as the column, the graph's `graph_width` wide.
    pub fn apply(self, cell: Div, graph_width: Rems) -> Div {
        match self {
            Column::Labels => grow(cell, 0.8, rems(0.)).min_w(rems(105. / 15.)),
            Column::Graph => cell.flex_none().w(graph_width),
            Column::Message => grow(cell, 2.2, rems(0.)).min_w(rems(240. / 15.)),
            Column::Date => cell.flex_none().w(DATE_WIDTH),
        }
    }
}

/// `cell` taking `share` of what's left of the row after every cell's `basis`.
fn grow(cell: Div, share: f32, basis: Rems) -> Div {
    cell.flex_basis(basis)
        .flex_grow(share)
        .flex_shrink_0()
        .min_w_0()
}

/// The id of an element `kind` of the row (or label) `key`: `("commit", sha)`, say. Stable as
/// the rows are reloaded and scrolled, unlike their places.
pub fn keyed(kind: &'static str, key: impl Into<SharedString>) -> ElementId {
    ElementId::NamedChild(Arc::new(ElementId::Name(kind.into())), key.into())
}

/// The id of `parent`'s part `part`.
pub fn part(parent: &ElementId, part: impl Into<SharedString>) -> ElementId {
    ElementId::NamedChild(Arc::new(parent.clone()), part.into())
}

/// `color` with `alpha` of it showing: CSS's `color-mix(in srgb, color X%, transparent)`.
pub fn tinted(color: Hsla, alpha: f32) -> Hsla {
    Hsla {
        a: color.a * alpha,
        ..color
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::hsla;

    use super::*;

    #[test]
    fn tints_a_color() {
        assert_eq!(tinted(hsla(0.5, 0.5, 0.5, 1.), 0.16).a, 0.16);
    }
}
