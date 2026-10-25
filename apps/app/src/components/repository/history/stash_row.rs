//! A stash in the history. Ported from `components/repository/history/stash-row.tsx`.

use std::rc::Rc;

use gitto_ui_logic::git::rows::StashRow;
use gpui_kit::prelude::*;
use gpui_kit::{
    App, FontWeight, Rems, SharedString, Window, div, linear_color_stop, linear_gradient,
};

use super::columns::{CELL_PADDING, Column, MESSAGE_TEXT, SMALL_TEXT, keyed, tinted};
use super::commit_row::RowProps;
use super::history_graph::{GraphNode, HistoryGraph};
use super::history_option::{HistoryOption, RowKindBadge, RowLook, RowTotals};
use crate::components::ui::IconName;
use crate::state::clock::relative_time;
use crate::theme::{Palette, PaletteExt as _};

/// A stash in the history: a box in the graph, with a dashed line to the commit it was made on.
#[derive(IntoElement)]
pub struct HistoryStashRow {
    stash: StashRow,
    props: RowProps,
}

impl HistoryStashRow {
    pub fn new(stash: StashRow, props: RowProps) -> Self {
        HistoryStashRow { stash, props }
    }
}

fn stash_look(selected: bool, palette: &Palette) -> RowLook {
    let amber = palette.amber;
    RowLook {
        background: selected.then(|| {
            linear_gradient(
                90.,
                linear_color_stop(tinted(amber, 0.18), 0.),
                linear_color_stop(tinted(amber, 0.05), 1.),
            )
        }),
        hover: (!selected).then_some(palette.panel_hover),
        border: palette.border_soft,
        dashed: false,
        accent: selected.then_some(amber),
        text: if selected {
            palette.text_soft
        } else {
            palette.muted
        },
    }
}

impl RenderOnce for HistoryStashRow {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = *cx.palette();
        let stash = self.stash;
        let props = self.props;
        let id: SharedString = stash.id.clone().into();
        let on_select = Rc::clone(&props.on_select);
        let graph = stash.graph.map(|row| {
            HistoryGraph::new(keyed("graph", id.clone()), row, GraphNode::Stash)
                .node_only(!props.needle.is_empty())
        });
        let message = div()
            .flex()
            .items_center()
            .justify_between()
            .gap(Rems(7. / 15.))
            .px(CELL_PADDING)
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .italic()
                    .text_size(MESSAGE_TEXT)
                    .font_weight(FontWeight(570.))
                    .text_color(palette.text_soft)
                    .child(stash.message.clone()),
            )
            .children(props.totals.map(RowTotals::new));
        let date = div()
            .flex()
            .items_center()
            .px(CELL_PADDING)
            .text_size(SMALL_TEXT)
            .child(div().truncate().child(relative_time(stash.created_at)));
        HistoryOption::new(
            keyed("stash", id),
            stash.message,
            props.position,
            stash_look(props.selected, &palette),
            move |window, cx| on_select(window, cx),
        )
        .selected(props.selected)
        .outlined(props.outlined)
        .cell(
            Column::Labels.apply(
                div()
                    .flex()
                    .items_center()
                    .px(CELL_PADDING)
                    .child(RowKindBadge::new(IconName::Archive, "Stash")),
                props.graph_width,
            ),
        )
        .graph_cell(Column::Graph.apply(
            div().h_full().overflow_hidden().children(graph),
            props.graph_width,
        ))
        .cell(Column::Message.apply(message, props.graph_width))
        .cell(Column::Date.apply(date, props.graph_width))
    }
}
