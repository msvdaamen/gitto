//! Selecting a viewer's code and copying it, in the diff's viewer and the conflicts': their rows
//! are one text, selected across the rows in view and those scrolled to, as the TS viewers' DOM
//! was. It's GPUI Kit's window text selection (`TextSelection`), with the viewer as one
//! participant whose places are its rows and the bytes of their lines, so a selection outlives
//! the rows that scroll out of view; and what's copied is the lines' text as it is in the file,
//! tabs and all of a line cut short, not as it's drawn (see `DrawnLine`). Side by side, a
//! selection is of one side's column, the one it started in.

use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::rc::Rc;

use gpui_kit::base::{
    GlobalState, TextSelection, TextSelectionContentKey, TextSelectionEvent, TextSelectionHandle,
    TextSelectionRegistration, TextSelectionRun,
};
use gpui_kit::{
    AnyElement, App, Bounds, ClipboardItem, Context, Element, ElementId, FocusHandle,
    GlobalElementId, HighlightStyle, Hitbox, HitboxBehavior, Hsla, InspectorElementId, IntoElement,
    LayoutId, Pixels, Point, StyledText, Subscription, TextLayout, UniformListScrollHandle,
    WeakEntity, Window, WindowId, combine_highlights, px,
};

use super::patch_viewer::DrawnLine;

/// A place in a viewer's rows: its row, and a byte of the line in it, as the line is in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::components::repository::diff) struct CodePoint {
    pub row: usize,
    pub offset: usize,
}

/// What's selected of a viewer's rows: from `start` to `end`, of their lines in `column`, the
/// only one but side by side, where the deletions' are 0 and the additions' 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::components::repository::diff) struct CodeRange {
    pub column: usize,
    pub start: CodePoint,
    pub end: CodePoint,
}

impl CodeRange {
    /// The bytes selected of the line in the row `row`, `len` bytes long (without its line end),
    /// if it's among those selected.
    pub fn in_line(&self, row: usize, len: usize) -> Option<Range<usize>> {
        if row < self.start.row || row > self.end.row {
            return None;
        }
        let start = if row == self.start.row {
            self.start.offset.min(len)
        } else {
            0
        };
        let end = if row == self.end.row {
            self.end.offset.min(len)
        } else {
            len
        };
        Some(start..end.max(start))
    }

    /// The text selected of `lines`, the line in each row of the range's column (`None` for a row
    /// without one there, such as a separator's), each without its line end: a line a row.
    pub fn text<'a>(&self, lines: impl Fn(usize) -> Option<&'a str>) -> String {
        let mut text = String::new();
        let mut first = true;
        for row in self.start.row..=self.end.row {
            let Some(line) = lines(row) else {
                continue;
            };
            let line = line.trim_end_matches(['\n', '\r']);
            let Some(range) = self.in_line(row, line.len()) else {
                continue;
            };
            if !first {
                text.push('\n');
            }
            first = false;
            text.push_str(
                &line[line.floor_char_boundary(range.start)..line.floor_char_boundary(range.end)],
            );
        }
        text
    }
}

/// A place as the window's selection keeps it (`TextSelectionContentKey`): the column in the top
/// bit, then the row in 31, then the byte in 32.
fn encode(column: usize, point: CodePoint) -> u64 {
    ((column as u64 & 1) << 63)
        | ((point.row as u64 & 0x7fff_ffff) << 32)
        | (point.offset.min(u32::MAX as usize) as u64)
}

fn decode(key: u64) -> (usize, CodePoint) {
    (
        (key >> 63) as usize,
        CodePoint {
            row: ((key >> 32) & 0x7fff_ffff) as usize,
            offset: (key & 0xffff_ffff) as usize,
        },
    )
}

/// A line of code as it was drawn this frame, where the selection can be in it.
struct Run {
    column: usize,
    row: usize,
    line: Rc<DrawnLine>,
    layout: TextLayout,
    bounds: Bounds<Pixels>,
}

/// Where the lines in view were drawn, and the view itself.
#[derive(Default)]
struct Geometry {
    bounds: Bounds<Pixels>,
    scroll_offset: Point<Pixels>,
    runs: Vec<Run>,
}

impl Geometry {
    /// The place nearest `point`, in the view's contents, among the lines of `column` (of any, if
    /// it's `None`).
    fn place_at(&self, point: Point<Pixels>, column: Option<usize>) -> Option<(usize, CodePoint)> {
        let point = point + self.bounds.origin + self.scroll_offset;
        let run = self
            .runs
            .iter()
            .filter(|run| column.is_none_or(|column| run.column == column))
            .map(|run| (distance(point, run.bounds), run))
            .min_by(|((a_off, a), _), ((b_off, b), _)| a_off.cmp(b_off).then(a.total_cmp(b)))
            .map(|(_, run)| run)?;
        let drawn = run
            .layout
            .index_for_position(point)
            .unwrap_or_else(|closest| closest);
        Some((
            run.column,
            CodePoint {
                row: run.row,
                offset: run.line.source_offset(drawn),
            },
        ))
    }
}

/// How far `point` is from `bounds`: whether it's off their line, then how far, squared.
fn distance(point: Point<Pixels>, bounds: Bounds<Pixels>) -> (bool, f32) {
    let off_line = point.y < bounds.top() || point.y >= bounds.bottom();
    let dx: f32 = (bounds.left() - point.x)
        .max(point.x - bounds.right())
        .max(px(0.))
        .into();
    let dy: f32 = (bounds.top() - point.y)
        .max(point.y - bounds.bottom())
        .max(px(0.))
        .into();
    (off_line, dx * dx + dy * dy)
}

/// A viewer's code, as one selectable text (see the module's doc): it takes part in the window's
/// selection while the viewer's on show, and says what's selected of it.
pub(in crate::components::repository::diff) struct CodeSelection {
    handle: TextSelectionHandle,
    geometry: Rc<RefCell<Geometry>>,
    /// The window it's drawn in, to clear its selection.
    window: Rc<Cell<Option<WindowId>>>,
    _changes: Subscription,
}

impl CodeSelection {
    /// The selection of the viewer `cx` is of, which `of` finds it in, focusing `focus` as it
    /// starts and copying what `copied` makes of it.
    pub fn new<V: 'static>(
        focus: &FocusHandle,
        of: fn(&V) -> &CodeSelection,
        copied: fn(&V, CodeRange) -> String,
        cx: &mut Context<V>,
    ) -> Self {
        let handle = TextSelectionHandle::new("", cx);
        let geometry = Rc::new(RefCell::new(Geometry::default()));
        let viewer: WeakEntity<V> = cx.weak_entity();
        handle.resolve_content_key_with(
            {
                let viewer = viewer.clone();
                let geometry = Rc::clone(&geometry);
                move |point, cx| {
                    let viewer = viewer.upgrade()?;
                    // Where it started: the selection stays in that column.
                    let column = of(viewer.read(cx)).anchor(cx).map(|(column, _)| column);
                    let (column, place) = geometry.borrow().place_at(point, column)?;
                    Some(TextSelectionContentKey::new(encode(column, place)))
                }
            },
            cx,
        );
        handle.copy_with(
            {
                let viewer = viewer.clone();
                move |cx| {
                    let Some(viewer) = viewer.upgrade() else {
                        return String::new();
                    };
                    let viewer = viewer.read(cx);
                    of(viewer)
                        .range(cx)
                        .map_or_else(String::new, |range| copied(viewer, range))
                }
            },
            cx,
        );
        handle.focus_with(
            {
                let focus = focus.clone();
                move |window, cx| window.focus(&focus, cx)
            },
            cx,
        );
        let changes = handle.subscribe(
            move |event, cx| {
                if matches!(
                    event,
                    TextSelectionEvent::SelectionChanged(_) | TextSelectionEvent::Cleared
                ) {
                    let _ = viewer.update(cx, |_, cx| cx.notify());
                }
            },
            cx,
        );
        CodeSelection {
            handle,
            geometry,
            window: Rc::default(),
            _changes: changes,
        }
    }

    /// Where the selection started, if it's in these rows.
    fn anchor(&self, cx: &App) -> Option<(usize, CodePoint)> {
        let snapshot = self.handle.snapshot(cx)?;
        let anchor = snapshot.anchor();
        (anchor.entity_id() == Some(self.handle.entity_id()))
            .then(|| anchor.content_key())
            .flatten()
            .map(|key| decode(key.value()))
    }

    /// What's selected of the rows, if anything is.
    pub fn range(&self, cx: &App) -> Option<CodeRange> {
        let snapshot = self.handle.snapshot(cx)?;
        let ours = Some(self.handle.entity_id());
        if snapshot.anchor().entity_id() != ours || snapshot.cursor().entity_id() != ours {
            return None;
        }
        let (column, anchor) = decode(snapshot.anchor().content_key()?.value());
        let (_, cursor) = decode(snapshot.cursor().content_key()?.value());
        (anchor != cursor).then(|| CodeRange {
            column,
            start: anchor.min(cursor),
            end: anchor.max(cursor),
        })
    }

    /// Copies what's selected of the rows, as `copied` makes it of them, and says whether
    /// anything was.
    pub fn copy<V>(&self, viewer: &V, copied: fn(&V, CodeRange) -> String, cx: &mut App) -> bool {
        let Some(range) = self.range(cx) else {
            return false;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(copied(viewer, range)));
        true
    }

    /// Lets go of what's selected of the rows, as they're laid out again: it's of the rows as
    /// they were.
    pub fn clear(&self, cx: &mut App) {
        let (Some(window), Some(_)) = (self.window.get(), self.handle.snapshot(cx)) else {
            return;
        };
        cx.defer(move |cx| TextSelection::clear_for_window(window, cx));
    }

    /// The rows, `list`, scrolled with `scroll`, as the selectable text.
    pub fn area(&self, list: AnyElement, scroll: &UniformListScrollHandle) -> SelectionArea {
        SelectionArea {
            list,
            scroll: scroll.clone(),
            handle: self.handle.clone(),
            geometry: Rc::clone(&self.geometry),
            window: Rc::clone(&self.window),
        }
    }

    /// What the rows draw their lines' text with, for this frame.
    pub fn texts(&self, cx: &App) -> CodeTexts {
        CodeTexts {
            geometry: Rc::clone(&self.geometry),
            selected: self.range(cx),
        }
    }

    /// Keeps the next mouse press from starting a selection: one on a line's number, which picks
    /// lines to stage instead.
    pub fn hold_off(cx: &mut App) {
        GlobalState::suppress_text_selection(cx);
    }
}

/// What a frame's rows draw their lines' text with (see [`CodeSelection::texts`]).
#[derive(Clone)]
pub(in crate::components::repository::diff) struct CodeTexts {
    geometry: Rc<RefCell<Geometry>>,
    selected: Option<CodeRange>,
}

impl CodeTexts {
    /// The text of the line in the row `row`'s column `column`, drawn as `line` is, with
    /// `highlights`, and what's selected of it in `color`.
    pub fn text(
        &self,
        column: usize,
        row: usize,
        line: Rc<DrawnLine>,
        highlights: Vec<(Range<usize>, HighlightStyle)>,
        color: Hsla,
    ) -> CodeText {
        let selected = self
            .selected
            .filter(|range| range.column == column)
            .and_then(|range| range.in_line(row, line.line_len()))
            .map(|range| line.offset(range.start)..line.drawn_end(range.end))
            .filter(|range| !range.is_empty())
            .map(|range| {
                let style = HighlightStyle {
                    background_color: Some(color),
                    ..HighlightStyle::default()
                };
                (range, style)
            });
        let highlights: Vec<_> = combine_highlights(highlights, selected).collect();
        CodeText {
            text: StyledText::new(line.text.clone()).with_highlights(highlights),
            column,
            row,
            line,
            geometry: Rc::clone(&self.geometry),
        }
    }
}

/// The rows of a viewer, as the text its selection is of (see [`CodeSelection::area`]): takes
/// part in the window's selection with the lines drawn in them.
pub(in crate::components::repository::diff) struct SelectionArea {
    list: AnyElement,
    scroll: UniformListScrollHandle,
    handle: TextSelectionHandle,
    geometry: Rc<RefCell<Geometry>>,
    window: Rc<Cell<Option<WindowId>>>,
}

impl IntoElement for SelectionArea {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for SelectionArea {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        Some(ElementId::NamedInteger(
            "code-selection".into(),
            self.handle.entity_id().as_u64(),
        ))
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.list.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Hitbox {
        {
            let mut geometry = self.geometry.borrow_mut();
            geometry.runs.clear();
            geometry.bounds = bounds;
        }
        // Behind the rows, so their numbers and buttons are pressed as they are.
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        self.list.prepaint(window, cx);
        let offset = self.scroll.0.borrow().base_handle.offset();
        self.geometry.borrow_mut().scroll_offset = offset;
        self.window.set(Some(window.window_handle().window_id()));
        let text_bounds = self
            .geometry
            .borrow()
            .runs
            .iter()
            .map(|run| run.bounds)
            .collect();
        self.handle.register(
            TextSelectionRegistration::new(hitbox.clone(), bounds)
                .with_scroll_offset(offset)
                .with_text_bounds(text_bounds)
                .with_rendered_element(&self.handle, window, cx),
            window,
            cx,
        );
        hitbox
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut Hitbox,
        window: &mut Window,
        cx: &mut App,
    ) {
        let runs: Vec<TextSelectionRun> = self
            .geometry
            .borrow()
            .runs
            .iter()
            .map(|run| {
                let key = encode(
                    run.column,
                    CodePoint {
                        row: run.row,
                        offset: 0,
                    },
                );
                TextSelectionRun::new(run.line.text.clone(), run.layout.clone(), run.bounds)
                    .with_document_order(key)
            })
            .collect();
        self.handle.update_runs(&runs, cx);
        self.list.paint(window, cx);
    }
}

/// A line's text, which tells the selection where it's drawn (see [`CodeSelection::text`]).
pub(in crate::components::repository::diff) struct CodeText {
    text: StyledText,
    column: usize,
    row: usize,
    line: Rc<DrawnLine>,
    geometry: Rc<RefCell<Geometry>>,
}

impl IntoElement for CodeText {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for CodeText {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        self.text.request_layout(id, inspector_id, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.text
            .prepaint(id, inspector_id, bounds, &mut (), window, cx);
        self.geometry.borrow_mut().runs.push(Run {
            column: self.column,
            row: self.row,
            line: Rc::clone(&self.line),
            layout: self.text.layout().clone(),
            bounds,
        });
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.text
            .paint(id, inspector_id, bounds, &mut (), &mut (), window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_a_place_as_the_window_selection_does() {
        for (column, row, offset) in [
            (0, 0, 0),
            (1, 12, 4000),
            (0, 0x7fff_ffff, u32::MAX as usize),
        ] {
            let point = CodePoint { row, offset };
            assert_eq!(decode(encode(column, point)), (column, point));
        }
        // In the order of the rows, then of the bytes in them.
        let at = |row, offset| encode(0, CodePoint { row, offset });
        assert!(at(1, 900) < at(2, 0));
    }

    #[test]
    fn copies_the_lines_selected_as_they_are_in_the_file() {
        let lines = ["\tone\n", "two\r\n", "three"];
        let range = CodeRange {
            column: 0,
            start: CodePoint { row: 0, offset: 1 },
            end: CodePoint { row: 2, offset: 3 },
        };
        let text = range.text(|row| lines.get(row).copied());
        assert_eq!(text, "one\ntwo\nthr");
        // Row 1 has no line in the column, e.g. a separator's.
        let rows = [Some("a\n"), None, Some("b\n")];
        let across = CodeRange {
            column: 0,
            start: CodePoint { row: 0, offset: 0 },
            end: CodePoint { row: 2, offset: 1 },
        };
        assert_eq!(across.text(|row| rows[row]), "a\nb");
    }
}
