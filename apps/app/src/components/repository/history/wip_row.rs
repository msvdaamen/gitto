//! The uncommitted changes in the history. Ported from
//! `components/repository/history/wip-row.tsx`.

use std::rc::Rc;

use gitto_git::features::status::schema::StatusCounts;
use gitto_ui::git::rows::{WIP_MESSAGE, WipRow};
use gpui_kit::component::h_flex;
use gpui_kit::prelude::*;
use gpui_kit::{App, FontWeight, SharedString, Window, div, linear_color_stop, linear_gradient};

use super::columns::{
    CELL_PADDING, COUNT_LINE, Column, LABEL_TEXT, MESSAGE_TEXT, SMALL_TEXT, mixed, tinted,
};
use super::commit_row::RowProps;
use super::history_graph::{GraphNode, HistoryGraph};
use super::history_option::{HistoryOption, RowKindBadge, RowLook};
use crate::components::ui::{IconName, Tone};
use crate::theme::{Palette, PaletteExt as _};

/// What the uncommitted changes' row says of them: how many files are staged, unstaged and
/// conflicted, and the branch they're on.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WipSummary {
    pub counts: Option<StatusCounts>,
    /// HEAD, as the status labels it: the branch, or "detached HEAD".
    pub branch: Option<SharedString>,
}

/// The uncommitted changes, set apart from the commits below: an amber, dashed-off row with a
/// hollow graph node that counts what's staged and what isn't.
#[derive(IntoElement)]
pub struct HistoryWipRow {
    row: WipRow,
    props: RowProps,
    summary: WipSummary,
}

impl HistoryWipRow {
    pub fn new(row: WipRow, props: RowProps, summary: WipSummary) -> Self {
        HistoryWipRow {
            row,
            props,
            summary,
        }
    }
}

fn wip_look(selected: bool, palette: &Palette) -> RowLook {
    let amber = palette.amber;
    RowLook {
        background: Some(if selected {
            linear_gradient(
                90.,
                linear_color_stop(tinted(amber, 0.24), 0.),
                linear_color_stop(tinted(amber, 0.08), 1.),
            )
        } else {
            tinted(palette.amber_soft, 0.7).into()
        }),
        hover: (!selected).then_some(palette.amber_soft),
        border: mixed(amber, 0.45, palette.border),
        dashed: true,
        accent: Some(amber),
        text: palette.muted,
    }
}

/// A count of the changes, in its tone.
fn wip_count(count: u64, what: &str, tone: Tone, palette: &Palette) -> Option<impl IntoElement> {
    let (background, foreground) = tone.colors(palette);
    (count > 0).then(|| {
        div()
            .flex_none()
            .rounded_full()
            .px_1p5()
            .py_0p5()
            .bg(background)
            .text_color(foreground)
            .text_size(LABEL_TEXT)
            .line_height(COUNT_LINE)
            .font_weight(FontWeight(680.))
            .child(format!("{count} {what}"))
    })
}

impl RenderOnce for HistoryWipRow {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = *cx.palette();
        let props = self.props;
        let counts = self.summary.counts.unwrap_or(StatusCounts {
            files: 0,
            staged: 0,
            unstaged: 0,
            conflicted: 0,
        });
        let on_select = Rc::clone(&props.on_select);
        let graph = self.row.graph.map(|row| {
            HistoryGraph::new("wip-graph", row, GraphNode::Wip).node_only(!props.needle.is_empty())
        });
        let message = h_flex()
            .gap_2()
            .px(CELL_PADDING)
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .italic()
                    .text_size(MESSAGE_TEXT)
                    .font_weight(FontWeight(620.))
                    .text_color(palette.text)
                    .child(WIP_MESSAGE),
            )
            .children(wip_count(counts.staged, "staged", Tone::Mint, &palette))
            .children(wip_count(
                counts.unstaged,
                "unstaged",
                Tone::Amber,
                &palette,
            ))
            .children(wip_count(
                counts.conflicted,
                "conflicted",
                Tone::Coral,
                &palette,
            ));
        // The TS's message spanned the date's column too, the branch at its end: here the
        // branch is in the date's column, so the row keeps the others' columns.
        let branch = h_flex()
            .justify_end()
            .px(CELL_PADDING)
            .text_size(SMALL_TEXT)
            .text_color(palette.faint)
            .child(div().truncate().child(format!(
                "on {}",
                self.summary.branch.as_deref().unwrap_or_default()
            )));
        HistoryOption::new(
            "wip",
            WIP_MESSAGE,
            props.position,
            wip_look(props.selected, &palette),
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
                    .child(RowKindBadge::new(IconName::PencilLine, "WIP")),
                props.graph_width,
            ),
        )
        .graph_cell(Column::Graph.apply(
            div().h_full().overflow_hidden().children(graph),
            props.graph_width,
        ))
        .cell(Column::Message.apply(message.overflow_hidden(), props.graph_width))
        .cell(Column::Date.apply(branch, props.graph_width))
    }
}
