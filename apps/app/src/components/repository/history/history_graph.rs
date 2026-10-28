//! A row of the history graph. Ported from `components/repository/history/history-graph.tsx`.
//!
//! The TS drew each row as an SVG. Here the lines are painted on a canvas, from the same paths
//! (see [`graph_lines`]), and the node is an element of its own, so a commit's can show its author
//! in a tooltip. The geometry is the TS's, in pixels of the 15px base, scaled with the window's
//! rem size so the graph zooms with the rest of the app.

use gitto_ui::git::graph::{GraphEdge, GraphRow};
use gpui_kit::prelude::*;
use gpui_kit::{
    App, BorderStyle, Bounds, ElementId, FontWeight, Hsla, PathBuilder, Pixels, Rems, SharedString,
    TestSupportExt as _, Window, canvas, div, point, px, quad, size,
};

use super::author_tooltip::{GraphAuthor, author_tooltip};
use super::columns::{ROW_HEIGHT, part};
use crate::theme::{Palette, PaletteExt as _};

/// Width of a lane, and the space left and right of the lanes.
const LANE: f32 = 20.;
const PADDING: f32 = 6.;
/// Height of a row's graph: the row, without the bottom border its other cells have, so a node
/// is in line with their text.
const HEIGHT: f32 = 38. - 1.;
const MIDDLE: f32 = HEIGHT / 2.;
const NODE_RADIUS: f32 = 8.;
/// Radius of the rounded corner where a line turns from one lane towards another.
const CORNER: f32 = 8.;
/// How thick the lines are, and a stash's box.
const STROKE: f32 = 2.;
const BOX_STROKE: f32 = 1.5;
/// The pixels of the 15px base the geometry is in.
const BASE: f32 = 15.;

/// The colour of a lane, shared by its line, its nodes and the labels of branches on it.
pub fn lane_color(lane: usize, palette: &Palette) -> Hsla {
    let colors = [
        palette.primary_strong,
        palette.blue,
        palette.mint,
        palette.amber,
        palette.coral,
    ];
    colors[lane % colors.len()]
}

/// Width of the graph column for rows that use at most `lanes` lanes.
pub fn graph_width(lanes: usize) -> Rems {
    let pixels = (lanes as f32 * LANE + PADDING * 2.).clamp(64., 260.);
    Rems(pixels / BASE)
}

fn x(lane: usize) -> f32 {
    PADDING + lane as f32 * LANE + LANE / 2.
}

/// A step of a line's path, in pixels of the 15px base (SVG's `M`, `L`/`V`/`H` and `Q`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathStep {
    MoveTo(f32, f32),
    LineTo(f32, f32),
    /// A quadratic curve to the point, bending towards the control point.
    CurveTo {
        to: (f32, f32),
        control: (f32, f32),
    },
}

/// A line of a row of the graph: its path, the lane whose colour it takes, and whether it's
/// dashed (amber, from the uncommitted changes to HEAD, or from a stash to its commit).
#[derive(Debug, Clone, PartialEq)]
pub struct GraphLine {
    pub path: Vec<PathStep>,
    pub lane: usize,
    pub dashed: bool,
}

/// A lane passing by the node, from the top of the row to the bottom.
fn through_path(edge: &GraphEdge) -> Vec<PathStep> {
    let from = x(edge.from);
    vec![
        PathStep::MoveTo(from, 0.),
        PathStep::LineTo(from, HEIGHT + 1.),
    ]
}

/// A line from the top of the row into the node: straight down, then a corner into the node.
fn top_path(edge: &GraphEdge) -> Vec<PathStep> {
    let from = x(edge.from);
    let to = x(edge.to);
    if from == to {
        return vec![PathStep::MoveTo(from, 0.), PathStep::LineTo(from, MIDDLE)];
    }
    let towards = (to - from).signum();
    vec![
        PathStep::MoveTo(from, 0.),
        PathStep::LineTo(from, MIDDLE - CORNER),
        PathStep::CurveTo {
            to: (from + towards * CORNER, MIDDLE),
            control: (from, MIDDLE),
        },
        PathStep::LineTo(to, MIDDLE),
    ]
}

/// A line from the node to the bottom of the row: across to its lane, then a corner down.
fn bottom_path(edge: &GraphEdge) -> Vec<PathStep> {
    let from = x(edge.from);
    let to = x(edge.to);
    // One past the row's other cells, to the row's bottom, as its cell has no bottom border: the
    // line meets the next row's.
    let bottom = HEIGHT + 1.;
    if from == to {
        return vec![
            PathStep::MoveTo(from, MIDDLE),
            PathStep::LineTo(from, bottom),
        ];
    }
    let towards = (to - from).signum();
    vec![
        PathStep::MoveTo(from, MIDDLE),
        PathStep::LineTo(to - towards * CORNER, MIDDLE),
        PathStep::CurveTo {
            to: (to, MIDDLE + CORNER),
            control: (to, MIDDLE),
        },
        PathStep::LineTo(to, bottom),
    ]
}

/// The lines of `row`: the lanes passing by, then those into the node, then those out of it. A
/// line takes its lane's colour: the lane it comes from, or for one out of the node, the lane it
/// goes to.
pub fn graph_lines(row: &GraphRow) -> Vec<GraphLine> {
    let line = |path, lane, edge: &GraphEdge| GraphLine {
        path,
        lane,
        dashed: edge.dashed,
    };
    row.through
        .iter()
        .map(|edge| line(through_path(edge), edge.from, edge))
        .chain(
            row.top
                .iter()
                .map(|edge| line(top_path(edge), edge.from, edge)),
        )
        .chain(
            row.bottom
                .iter()
                .map(|edge| line(bottom_path(edge), edge.to, edge)),
        )
        .collect()
}

/// What the node of a row is.
#[derive(Debug, Clone, PartialEq)]
pub enum GraphNode {
    /// A commit: an avatar ringed in its lane's colour, named for its author; a merge is a small
    /// dot.
    Commit(GraphAuthor),
    /// The uncommitted changes: a dashed, hollow circle.
    Wip,
    /// A stash: a box with its lid.
    Stash,
}

/// A commit's row of the history graph. Commits are avatars ringed in their lane's colour, merges
/// small dots, the uncommitted changes a dashed, hollow circle with a dashed line to HEAD, and a
/// stash a box with a dashed line to the commit it was made on. A commit's node shows its author
/// when hovered.
#[derive(IntoElement)]
pub struct HistoryGraph {
    id: ElementId,
    row: GraphRow,
    node: GraphNode,
    /// Leave out the lines, e.g. while searching, when the rows they lead to may be hidden.
    node_only: bool,
}

impl HistoryGraph {
    pub fn new(id: impl Into<ElementId>, row: GraphRow, node: GraphNode) -> Self {
        HistoryGraph {
            id: id.into(),
            row,
            node,
            node_only: false,
        }
    }

    pub fn node_only(mut self, node_only: bool) -> Self {
        self.node_only = node_only;
        self
    }
}

impl RenderOnce for HistoryGraph {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = *cx.palette();
        // Pixels per pixel of the 15px base.
        let scale = f32::from(window.rem_size()) / BASE;
        let column = self.row.column;
        let is_merge = self.row.bottom.len() > 1;
        let amber = palette.amber;
        let color = match self.node {
            GraphNode::Commit(_) => lane_color(column, &palette),
            GraphNode::Wip | GraphNode::Stash => amber,
        };
        let lines = if self.node_only {
            Vec::new()
        } else {
            graph_lines(&self.row)
        };
        let node = self.node.clone();
        let background = palette.bg;
        let painted = canvas(
            |_, _, _| {},
            move |bounds, (), window, _| {
                for line in &lines {
                    let line_color = if line.dashed {
                        amber
                    } else {
                        lane_color(line.lane, &palette)
                    };
                    paint_line(&line.path, line.dashed, line_color, bounds, scale, window);
                }
                match node {
                    GraphNode::Wip => paint_wip(column, color, background, bounds, scale, window),
                    GraphNode::Stash => {
                        paint_stash(column, color, background, bounds, scale, window)
                    }
                    GraphNode::Commit(_) => {}
                }
            },
        )
        .absolute()
        .inset_0()
        .size_full();

        let node = match self.node {
            GraphNode::Commit(author) => Some(commit_node(
                self.id.clone(),
                author,
                column,
                is_merge,
                color,
                scale,
                &palette,
            )),
            GraphNode::Wip | GraphNode::Stash => None,
        };
        div()
            .id(self.id)
            .relative()
            .w_full()
            .h(ROW_HEIGHT)
            .overflow_hidden()
            .child(painted)
            .children(node)
    }
}

/// The point at `(x, y)`, in pixels of the 15px base, in `bounds`.
fn at(bounds: Bounds<Pixels>, scale: f32, (x, y): (f32, f32)) -> gpui_kit::Point<Pixels> {
    bounds.origin + point(px(x * scale), px(y * scale))
}

fn paint_line(
    path: &[PathStep],
    dashed: bool,
    color: Hsla,
    bounds: Bounds<Pixels>,
    scale: f32,
    window: &mut Window,
) {
    let mut builder = PathBuilder::stroke(px(STROKE * scale));
    if dashed {
        builder = builder.dash_array(&[px(3. * scale), px(3. * scale)]);
    }
    for step in path {
        match *step {
            PathStep::MoveTo(x, y) => builder.move_to(at(bounds, scale, (x, y))),
            PathStep::LineTo(x, y) => builder.line_to(at(bounds, scale, (x, y))),
            PathStep::CurveTo { to, control } => {
                builder.curve_to(at(bounds, scale, to), at(bounds, scale, control))
            }
        }
    }
    if let Ok(path) = builder.build() {
        window.paint_path(path, color);
    }
}

/// The uncommitted changes' node: a dashed, hollow circle.
fn paint_wip(
    column: usize,
    color: Hsla,
    background: Hsla,
    bounds: Bounds<Pixels>,
    scale: f32,
    window: &mut Window,
) {
    // The SVG's circle of radius 7, its 2px stroke centred on it.
    let outer = (NODE_RADIUS - 1. + STROKE / 2.) * scale;
    let center = at(bounds, scale, (x(column), MIDDLE));
    let circle = Bounds::new(
        center - point(px(outer), px(outer)),
        size(px(outer * 2.), px(outer * 2.)),
    );
    window.paint_quad(quad(
        circle,
        px(outer),
        background,
        px(STROKE * scale),
        color,
        BorderStyle::Dashed,
    ));
}

/// A stash's node: a box with its lid.
fn paint_stash(
    column: usize,
    color: Hsla,
    background: Hsla,
    bounds: Bounds<Pixels>,
    scale: f32,
    window: &mut Window,
) {
    let node = x(column);
    let half = NODE_RADIUS - 1. + BOX_STROKE / 2.;
    let corner = at(bounds, scale, (node - half, MIDDLE - half));
    let side = px(half * 2. * scale);
    window.paint_quad(quad(
        Bounds::new(corner, size(side, side)),
        px(3. * scale),
        background,
        px(BOX_STROKE * scale),
        color,
        BorderStyle::Solid,
    ));
    let mut lid = PathBuilder::stroke(px(BOX_STROKE * scale));
    lid.move_to(at(bounds, scale, (node - NODE_RADIUS + 1., MIDDLE - 2.)));
    lid.line_to(at(bounds, scale, (node + NODE_RADIUS - 1., MIDDLE - 2.)));
    if let Ok(path) = lid.build() {
        window.paint_path(path, color);
    }
}

/// A commit's node, which shows its author when hovered: an avatar of their initials ringed in
/// the lane's colour, or a small dot for a merge.
fn commit_node(
    id: ElementId,
    author: GraphAuthor,
    column: usize,
    is_merge: bool,
    color: Hsla,
    scale: f32,
    palette: &Palette,
) -> impl IntoElement {
    let center = (x(column), MIDDLE);
    // The SVG's circles: an avatar of radius 8 with its 2px ring centred on it, or a dot of 4.5.
    let radius = if is_merge {
        4.5
    } else {
        NODE_RADIUS + STROKE / 2.
    };
    let initials: SharedString = author.initials.clone();
    div()
        .id(part(&id, "node"))
        .test_support()
        .absolute()
        .left(px((center.0 - radius) * scale))
        .top(px((center.1 - radius) * scale))
        .size(px(radius * 2. * scale))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .map(|this| {
            if is_merge {
                this.bg(color)
            } else {
                this.bg(author.color.unwrap_or(color))
                    .border(px(STROKE * scale))
                    .border_color(color)
                    .text_color(palette.avatar_text)
                    .text_size(px(7. * scale))
                    .line_height(px(7. * scale))
                    .font_weight(FontWeight::BOLD)
                    .child(initials)
            }
        })
        .tooltip(author_tooltip(author))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::base_px;

    /// A path as the TS's SVG `d` attribute spelled it.
    fn svg(path: &[PathStep]) -> String {
        let mut d = String::new();
        let mut last = (0., 0.);
        for step in path {
            match *step {
                PathStep::MoveTo(x, y) => d.push_str(&format!("M{x} {y}")),
                PathStep::LineTo(x, y) if x == last.0 => d.push_str(&format!("V{y}")),
                PathStep::LineTo(x, y) if y == last.1 => d.push_str(&format!("H{x}")),
                PathStep::LineTo(x, y) => d.push_str(&format!("L{x} {y}")),
                PathStep::CurveTo { to, control } => {
                    d.push_str(&format!("Q{} {} {} {}", control.0, control.1, to.0, to.1))
                }
            }
            last = match *step {
                PathStep::MoveTo(x, y) | PathStep::LineTo(x, y) => (x, y),
                PathStep::CurveTo { to, .. } => to,
            };
        }
        d
    }

    fn edge(from: usize, to: usize) -> GraphEdge {
        GraphEdge {
            from,
            to,
            dashed: false,
        }
    }

    #[test]
    fn draws_the_paths_the_svg_did() {
        assert_eq!(svg(&through_path(&edge(1, 1))), "M36 0V38");
        assert_eq!(svg(&top_path(&edge(0, 0))), "M16 0V18.5");
        assert_eq!(svg(&top_path(&edge(0, 2))), "M16 0V10.5Q16 18.5 24 18.5H56");
        assert_eq!(svg(&top_path(&edge(2, 0))), "M56 0V10.5Q56 18.5 48 18.5H16");
        assert_eq!(svg(&bottom_path(&edge(0, 0))), "M16 18.5V38");
        assert_eq!(
            svg(&bottom_path(&edge(0, 1))),
            "M16 18.5H28Q36 18.5 36 26.5V38"
        );
    }

    #[test]
    fn colours_a_line_out_of_the_node_by_the_lane_it_goes_to() {
        let row = GraphRow {
            column: 0,
            through: vec![edge(2, 2)],
            top: vec![GraphEdge {
                from: 0,
                to: 0,
                dashed: true,
            }],
            bottom: vec![edge(0, 0), edge(0, 1)],
            width: 3,
        };
        let lines = graph_lines(&row);
        let lanes: Vec<_> = lines.iter().map(|line| (line.lane, line.dashed)).collect();
        assert_eq!(lanes, [(2, false), (0, true), (0, false), (1, false)]);
    }

    #[test]
    fn sizes_the_graph_to_its_lanes_within_bounds() {
        assert_eq!(graph_width(1), base_px(64.));
        assert_eq!(graph_width(4), base_px(92.));
        assert_eq!(graph_width(40), base_px(260.));
    }
}
