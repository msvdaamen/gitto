//! A commit in the history. Ported from `components/repository/history/commit-row.tsx`.

use std::rc::Rc;

use gitto_ui::git::rows::CommitRow;
use gpui_kit::component::Colorize as _;
use gpui_kit::prelude::*;
use gpui_kit::{
    App, FontWeight, Hsla, Rems, SharedString, Window, div, linear_color_stop, linear_gradient,
};

use super::author_tooltip::GraphAuthor;
use super::columns::{CELL_PADDING, Column, MESSAGE_TEXT, SMALL_TEXT, keyed, tinted};
use super::history_graph::{GraphNode, HistoryGraph, lane_color};
use super::history_option::{
    HistoryOption, MenuBuilder, OnSelect, RowLook, RowPosition, RowTotals,
};
use super::ref_labels::{HistoryRefLabels, LabelActions, MoreLabels};
use crate::state::clock::relative_time;
use crate::theme::{Palette, PaletteExt as _};
use gitto_ui::git::changes::LineCounts;

/// What every row gets from the table.
#[derive(Clone)]
pub struct RowProps {
    pub position: RowPosition,
    pub selected: bool,
    /// Whether the selected row is outlined: the keyboard moved the selection while the list has
    /// the focus.
    pub outlined: bool,
    pub graph_width: Rems,
    /// The lowercase search, if any. The graph then only shows the row's node, as its lines would
    /// lead to rows the search hides.
    pub needle: SharedString,
    /// The lines the row's commit or stash added and removed, once the details show the row and
    /// its files are loaded (see `RowTotals`).
    pub totals: Option<LineCounts>,
    pub on_select: OnSelect,
}

/// A commit in the history: its branches and tags, graph node (named for its author), message
/// and date. Right-clicking it opens its menu, if it has one (see `CommitMenu`); one of its
/// branches', right-clicking that.
#[derive(IntoElement)]
pub struct HistoryCommitRow {
    commit: CommitRow,
    props: RowProps,
    labels: LabelActions,
    more: Option<MoreLabels>,
    /// Its menu: only the last commit of the checked-out branch has one.
    menu: Option<MenuBuilder>,
    /// Whether its menu is open.
    menu_open: bool,
}

impl HistoryCommitRow {
    pub fn new(commit: CommitRow, props: RowProps, labels: LabelActions) -> Self {
        HistoryCommitRow {
            commit,
            props,
            labels,
            more: None,
            menu: None,
            menu_open: false,
        }
    }

    pub fn more(mut self, more: Option<MoreLabels>) -> Self {
        self.more = more;
        self
    }

    pub fn menu(mut self, menu: Option<MenuBuilder>, open: bool) -> Self {
        self.menu = menu;
        self.menu_open = open;
        self
    }
}

/// How a commit's row looks: selected, it's tinted in the primary colour.
fn commit_look(selected: bool, menu_open: bool, palette: &Palette) -> RowLook {
    let background = if selected {
        Some(linear_gradient(
            90.,
            linear_color_stop(palette.primary_soft, 0.),
            linear_color_stop(tinted(palette.primary_soft, 0.35), 1.),
        ))
    } else if menu_open {
        Some(palette.panel_hover.into())
    } else {
        None
    };
    RowLook {
        background,
        hover: (!selected).then_some(palette.panel_hover),
        border: palette.border_soft,
        dashed: false,
        accent: selected.then_some(palette.primary),
        text: if selected {
            palette.text_soft
        } else {
            palette.muted
        },
    }
}

/// An author's avatar colour, from the hex `rows.ts` picked.
pub fn author_color(hex: &str) -> Option<Hsla> {
    Hsla::parse_hex(hex).ok()
}

impl RenderOnce for HistoryCommitRow {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = *cx.palette();
        let commit = self.commit;
        let props = self.props;
        let id: SharedString = commit.id.clone().into();
        let lane = commit.graph.as_ref().map_or(0, |graph| graph.column);
        let on_select = Rc::clone(&props.on_select);
        let labels = HistoryRefLabels::new(
            keyed("labels", id.clone()),
            commit.labels,
            props.needle.clone(),
            lane_color(lane, &palette),
            self.labels,
        )
        .when_some(self.more, HistoryRefLabels::more);
        let graph = commit.graph.map(|row| {
            HistoryGraph::new(
                keyed("graph", id.clone()),
                row,
                GraphNode::Commit(GraphAuthor {
                    name: commit.author.clone().into(),
                    initials: commit.initials.into(),
                    color: author_color(commit.avatar_color),
                }),
            )
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
                    .text_size(MESSAGE_TEXT)
                    .font_weight(FontWeight(570.))
                    .text_color(palette.text)
                    .child(commit.message.clone()),
            )
            .children(props.totals.map(RowTotals::new));
        let date = div()
            .flex()
            .items_center()
            .px(CELL_PADDING)
            .text_size(SMALL_TEXT)
            .child(div().truncate().child(relative_time(commit.committed_at)));
        let option = HistoryOption::new(
            keyed("commit", id),
            format!("{}, by {}", commit.message, commit.author),
            props.position,
            commit_look(props.selected, self.menu_open, &palette),
            move |window, cx| on_select(window, cx),
        )
        .selected(props.selected)
        .outlined(props.outlined)
        .cell(Column::Labels.apply(
            div().flex().items_center().px(CELL_PADDING).child(labels),
            props.graph_width,
        ))
        .graph_cell(Column::Graph.apply(
            div().h_full().overflow_hidden().children(graph),
            props.graph_width,
        ))
        .cell(Column::Message.apply(message, props.graph_width))
        .cell(Column::Date.apply(date, props.graph_width));
        match self.menu {
            Some(menu) => option.menu(move |popup, window, cx| menu(popup, window, cx)),
            None => option,
        }
    }
}
