//! The diff viewer: one file's patch, one column or side by side, highlighted, with only the rows
//! in view drawn. Ported from `patch-viewer.tsx`, which drew with @pierre/diffs; this draws its
//! rows itself (see `viewer_rows`), in GPUI's `uniform_list`, so a patch of tens of thousands of
//! lines costs as much as a screenful, and its highlighting is worked out off the main thread (see
//! `viewer_runtime`).

use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use gitto_git::features::staging::schema::LineSelection;
use gitto_ui::diff::diff_metadata::{ChangeType, FileDiff, Side};
use gitto_ui::diff::line_staging::{ChangedLine, SelectedRange, hunk_buttons, to_selection};
use gitto_ui::diff::patch_files::{Expanded, carried_expansion};
use gitto_ui::diff::viewer_rows::{
    ExpandDirection, Layout, LayoutOptions, LineAt, LineKind, Row, SeparatorButtons, Separators,
    expand, gap_size, layout,
};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::scroll::Scrollbar;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _, h_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Context, EventEmitter, FocusHandle, Focusable, HighlightStyle, Hsla,
    ListHorizontalSizingBehavior, MouseButton, MouseDownEvent, MouseMoveEvent, Pixels,
    ScrollStrategy, SharedString, Size, StyledText, Subscription, Task, TextRun,
    UniformListScrollHandle, WeakEntity, Window, combine_highlights, div, font, point, px, rems,
    uniform_list,
};

use super::highlight::{DiffHighlights, highlight_name};
use super::line_picker::{KeyOutcome, LinePicker, LineStaging, PickerKey, StagedAt, Viewport};
use super::viewer_actions::{
    ClearSelection, ExtendSelectionDown, ExtendSelectionUp, PATCH_VIEWER, ScrollLeft,
    ScrollPageDown, ScrollPageUp, ScrollRight, ScrollToBottom, ScrollToTop, SelectDown,
    SelectNextHunk, SelectPreviousHunk, SelectUp, StageSelection,
};
use super::viewer_runtime::{
    HIGHLIGHT_WAIT, LoadFile, SharedHighlights, ViewHighlights, ViewerRuntime, load_files,
};
use crate::components::ui::{Icon, IconName};
use crate::data::staging::StageAction;
use crate::state::diff_style::DiffStyle;
use crate::theme::{Palette, PaletteExt as _};

/// The changes' font size and line height, as the TS viewer had them (12px on 20px lines, at the
/// app's 15px base).
pub(super) const FONT_REMS: f32 = 0.8;
pub(super) const ROW_REMS: f32 = 4. / 3.;

/// Bytes of a line that are drawn: a longer one (a minified file's) is cut there, as shaping it
/// whole would hold up every frame it's in view.
const MAX_DRAWN_LINE: usize = 4000;

/// The spaces a tab is drawn as.
const TAB_WIDTH: usize = 4;

/// A line of code as it's drawn, here and in the conflicts' viewer: its line end left off, tabs
/// as spaces, a long one cut (with an ellipsis).
pub(super) struct DrawnLine {
    pub text: String,
    /// The bytes of the line that are drawn.
    cut: usize,
    /// Where its tabs are in the line.
    tabs: Vec<usize>,
}

impl DrawnLine {
    pub fn new(line: &str) -> Self {
        let content = line.trim_end_matches(['\n', '\r']);
        let cut = &content[..content.floor_char_boundary(MAX_DRAWN_LINE)];
        let ellipsis = cut.len() < content.len();
        let tabs: Vec<usize> = cut
            .bytes()
            .enumerate()
            .filter(|(_, byte)| *byte == b'\t')
            .map(|(offset, _)| offset)
            .collect();
        let mut text = if tabs.is_empty() {
            cut.to_owned()
        } else {
            cut.replace('\t', &" ".repeat(TAB_WIDTH))
        };
        if ellipsis {
            text.push('…');
        }
        DrawnLine {
            text,
            cut: cut.len(),
            tabs,
        }
    }

    /// Where a byte offset in the line is in the text drawn.
    pub fn offset(&self, offset: usize) -> usize {
        let offset = offset.min(self.cut);
        offset + self.tabs.partition_point(|tab| *tab < offset) * (TAB_WIDTH - 1)
    }

    /// The line's syntax, as `highlights` have it for line `ix` of `side`, where it's drawn.
    pub fn syntax<'a>(
        &'a self,
        highlights: &'a DiffHighlights,
        side: Side,
        ix: usize,
        cx: &'a App,
    ) -> impl Iterator<Item = (Range<usize>, HighlightStyle)> + 'a {
        let theme = &cx.theme().highlight_theme;
        highlights
            .syntax(side, ix)
            .iter()
            .filter_map(move |(range, name)| {
                let style = theme.style(highlight_name(*name))?;
                let range = self.offset(range.start as usize)..self.offset(range.end as usize);
                (!range.is_empty()).then_some((range, style))
            })
    }
}

/// The widest of `lines`, in characters as drawn (see [`DrawnLine`]).
pub(super) fn widest_line<'a>(lines: impl IntoIterator<Item = &'a str>) -> usize {
    lines
        .into_iter()
        .map(|line| {
            // As `DrawnLine` draws it: cut, with an ellipsis, and its tabs as spaces.
            let line = line.trim_end_matches(['\n', '\r']);
            let cut = &line[..line.floor_char_boundary(MAX_DRAWN_LINE)];
            let tabs = cut.bytes().filter(|byte| *byte == b'\t').count();
            cut.chars().count() + usize::from(cut.len() < line.len()) + tabs * (TAB_WIDTH - 1)
        })
        .max()
        .unwrap_or(0)
}

/// The width of a character of the code's font, at `font_size`.
pub(super) fn code_char_width(font_size: Pixels, window: &mut Window, cx: &App) -> Pixels {
    let sample: SharedString = "0000000000".into();
    let run = TextRun {
        len: sample.len(),
        font: font(cx.theme().mono_font_family.clone()),
        color: cx.theme().foreground,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window
        .text_system()
        .shape_line(sample, font_size, &[run], None)
        .width
        / 10.
}

/// Rows kept in sight above and below a line the view scrolls to, as moving through a list does.
const REVEAL_MARGIN_ROWS: f32 = 3.;

/// Which way the keyboard scrolls the view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scroll {
    LineDown,
    LineUp,
    Left,
    Right,
    PageDown,
    PageUp,
    Top,
    Bottom,
}

/// How far a list is scrolled, and how large its view and its contents were when it was last laid
/// out. The one place the viewer reads GPUI's scroll state by hand, as its handle scrolls to a row,
/// but neither by an amount nor with rows in sight on both sides of it.
struct ListScroll {
    /// How far it's scrolled down and right.
    top: Pixels,
    left: Pixels,
    /// `None` until it's laid out.
    view: Option<Size<Pixels>>,
    contents_width: Pixels,
}

impl ListScroll {
    fn of(scroll: &UniformListScrollHandle) -> Self {
        let state = scroll.0.borrow();
        let offset = state.base_handle.offset();
        ListScroll {
            top: -offset.y,
            left: -offset.x,
            view: state.last_item_size.map(|size| size.item),
            contents_width: state
                .last_item_size
                .map_or(px(0.), |size| size.contents.width),
        }
    }

    /// The view's height, or nothing before it's laid out.
    fn height(&self) -> Pixels {
        self.view.map_or(px(0.), |view| view.height)
    }
}

/// Scrolls the list `scroll` tracks to `left` and `top`, as they are: within its contents.
fn scroll_list_to(scroll: &UniformListScrollHandle, left: Pixels, top: Pixels) {
    scroll.0.borrow().base_handle.set_offset(point(-left, -top));
}

/// Scrolls the list `scroll` tracks, of `rows` rows `row_height` high, by `down` pixels (up, if
/// negative) and `across` right, within its contents.
pub(super) fn scroll_list_by(
    scroll: &UniformListScrollHandle,
    rows: usize,
    down: Pixels,
    across: Pixels,
    row_height: Pixels,
) {
    let at = ListScroll::of(scroll);
    let width = at.view.map_or(px(0.), |view| view.width);
    let bottom = (row_height * rows as f32 - at.height()).max(px(0.));
    let right = (at.contents_width - width).max(px(0.));
    let top = (at.top + down).clamp(px(0.), bottom);
    let left = (at.left + across).clamp(px(0.), right);
    scroll_list_to(scroll, left, top);
}

/// What the viewer tells the file's view.
#[derive(Debug, Clone, PartialEq)]
pub enum PatchViewerEvent {
    /// The last patch given is on show (`onShown`), with the key of the file it's of.
    Shown { file_key: String },
    /// Whether the whole file is being loaded, to show more of it (`onLoadingFiles`).
    LoadingFiles(bool),
    /// Why the whole file couldn't be loaded, e.g. it's too large; `None` if it could
    /// (`onFilesError`).
    FilesError(Option<String>),
    /// Why the last patch given can't be shown: it couldn't be parsed.
    Failed { file_key: String, message: String },
    /// Stages (or unstages, as `LineStaging::action` says) `lines` of `patch`, the patch on show
    /// they were picked from. The owner says if they couldn't be
    /// ([`stage_failed`](PatchViewer::stage_failed)).
    StageLines {
        patch: Arc<str>,
        lines: LineSelection,
    },
}

/// What the viewer shows, from its owner.
#[derive(Clone)]
pub struct PatchProps {
    /// The patch, as git gave it.
    pub patch: Arc<str>,
    /// Names the file the patch is of: a new patch with the same name is of the same file.
    pub file_key: String,
    /// Names the patch in the highlighting cache, e.g. by its commit and path: another patch,
    /// even of the same file, needs another name, or it's shown with the last one's highlighting.
    pub cache_key: String,
    pub style: DiffStyle,
    /// Stages or unstages lines picked in the patch on show.
    pub staging: Option<LineStaging>,
}

/// The patch to show: the last one given, which the one on show makes way for.
#[derive(Clone)]
struct NextPatch {
    patch: Arc<str>,
    file_key: String,
    cache_key: String,
}

impl PartialEq for NextPatch {
    fn eq(&self, other: &Self) -> bool {
        self.file_key == other.file_key
            && self.cache_key == other.cache_key
            && (Arc::ptr_eq(&self.patch, &other.patch) || self.patch == other.patch)
    }
}

/// The diff on show, and the file it's of.
struct Shown {
    diff: Arc<FileDiff>,
    patch: NextPatch,
    highlights: SharedHighlights,
    /// The widest line, in characters as drawn, and the highest line number.
    widest: usize,
    highest: usize,
}

/// One file's patch, as git gave it, with only the rows in view drawn. Only the changes and the
/// lines around them are in the patch; the rest of the file is read with the viewer's
/// `LoadFile`, by the object names on the patch's `index` line, once the lines between are asked
/// for.
///
/// Given another patch, it keeps the last one on show until the new one is highlighted (see
/// `HIGHLIGHT_WAIT`), then shows it in its place and says so ([`PatchViewerEvent::Shown`]).
/// Another file's starts at its top, with none of its lines shown around the changes; a new patch
/// of the same file, which uncommitted changes get whenever it's saved, takes the place of the
/// last where it's scrolled to, with the same lines shown around its changes. What it says of
/// loading the whole file is of the patch on show.
///
/// With staging, the lines of an uncommitted file's patch can be picked to stage or unstage: by
/// selecting them, with the mouse (on their numbers) or with the arrow keys (Shift to extend), or
/// a hunk at a time, with its button or `[` and `]`. The button in the gutter, by the changed line
/// the pointer is on or the last one selected, or Space stages them; Esc clears the selection.
/// Once the keyboard staged some, the selection moves to the change that's in their place.
pub struct PatchViewer {
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
    load_file: LoadFile,
    next: Option<NextPatch>,
    style: DiffStyle,
    staging: Option<LineStaging>,
    shown: Option<Shown>,
    /// The lines shown around each hunk of the diff on show.
    expanded: Expanded,
    layout: Rc<Layout>,
    /// Loads of the whole file under way, by the diff they're for.
    loads: Vec<Arc<FileDiff>>,
    /// Why the whole file couldn't be loaded, by the diff it's of: it's no use trying again.
    failures: Vec<(Arc<FileDiff>, String)>,
    picker: LinePicker,
    /// Where the keyboard staged the lines asked for last, if it did, until they're staged.
    staging_at: Option<StagedAt>,
    /// A line-number drag is selecting lines, from the selection's start.
    dragging: bool,
    /// The diffs this view had highlighted, to drop the long ones once it closes.
    highlights: ViewHighlights,
    /// What was last said of loading the whole file.
    reported: (bool, Option<String>),
    /// The width of a character of the code's font, by the font size it's for.
    char_width: Option<(Pixels, Pixels)>,
    /// The rows' height, as last drawn.
    row_height: Pixels,
    showing: Option<Task<()>>,
    highlighting: Option<Task<()>>,
    _release: Subscription,
}

impl EventEmitter<PatchViewerEvent> for PatchViewer {}

impl Focusable for PatchViewer {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl PatchViewer {
    pub fn new(load_file: LoadFile, cx: &mut Context<Self>) -> Self {
        let release = cx.on_release(|this: &mut Self, cx| this.highlights.drop_long(cx));
        PatchViewer {
            // Focusable, to scroll it with the keyboard, and pick lines to stage with it.
            focus: cx.focus_handle().tab_stop(true),
            scroll: UniformListScrollHandle::new(),
            load_file,
            next: None,
            style: DiffStyle::Unified,
            staging: None,
            shown: None,
            expanded: Expanded::new(),
            layout: Rc::default(),
            loads: Vec::new(),
            failures: Vec::new(),
            picker: LinePicker::default(),
            staging_at: None,
            dragging: false,
            highlights: ViewHighlights::default(),
            reported: (false, None),
            char_width: None,
            row_height: px(20.),
            showing: None,
            highlighting: None,
            _release: release,
        }
    }

    /// The diff on show, if one is.
    pub fn shown_diff(&self) -> Option<&Arc<FileDiff>> {
        self.shown.as_ref().map(|shown| &shown.diff)
    }

    /// The patch on show, as git gave it.
    pub fn shown_patch(&self) -> Option<&str> {
        self.shown.as_ref().map(|shown| &*shown.patch.patch)
    }

    /// Reads the file on show whole with `load_file` from now on.
    pub fn set_load_file(&mut self, load_file: LoadFile) {
        self.load_file = load_file;
    }

    /// Takes what's to be shown: another patch is shown once it's highlighted; another layout, or
    /// staging, at once.
    pub fn set_props(&mut self, props: PatchProps, cx: &mut Context<Self>) {
        let next = NextPatch {
            patch: props.patch,
            file_key: props.file_key,
            cache_key: props.cache_key,
        };
        let mut relayout = false;
        if self.style != props.style {
            self.style = props.style;
            relayout = true;
        }
        if self.staging != props.staging {
            let could = self.is_pickable();
            self.staging = props.staging;
            // Lines can be picked, or no longer: the hunks' buttons, and selecting lines, come and
            // go with that, on the patch on show. Not another file's, still on show while the next
            // one's is on its way.
            if could != self.is_pickable() && self.is_live() {
                if !self.is_pickable() {
                    self.picker.select(None, &self.layout);
                }
                relayout = true;
            }
            cx.notify();
        }
        if relayout {
            self.relayout();
            cx.notify();
        }
        match &mut self.next {
            Some(kept) if *kept == next => {
                // The same patch, loaded again: kept as it came, so it's told from the next one
                // without comparing them.
                kept.patch = next.patch;
            }
            _ => {
                self.next = Some(next.clone());
                self.show(next, cx);
            }
        }
    }

    /// Whether lines can be picked to stage.
    fn is_pickable(&self) -> bool {
        self.staging.is_some()
    }

    /// Whether the patch on show is the one lines are picked from: not the last file's, while the
    /// next one's is on its way, which staging isn't of.
    fn is_live(&self) -> bool {
        match (&self.shown, &self.next) {
            (Some(shown), Some(next)) => shown.patch.file_key == next.file_key,
            _ => false,
        }
    }

    /// Whether another patch is on its way, to take the place of the one on show.
    fn is_awaiting_patch(&self) -> bool {
        match (&self.shown, &self.next) {
            (Some(shown), Some(next)) => shown.patch != *next,
            (None, Some(_)) => true,
            _ => false,
        }
    }

    /// Shows `next`, once it's parsed and highlighted, unless it's passed over for another
    /// meanwhile.
    fn show(&mut self, next: NextPatch, cx: &mut Context<Self>) {
        let parsing = ViewerRuntime::parse(Arc::clone(&next.patch), next.cache_key.clone(), cx);
        self.showing = Some(cx.spawn(async move |this, cx| {
            let diff = match parsing.await {
                Ok(diff) => diff,
                Err(error) => {
                    let _ = this.update(cx, |this, cx| {
                        if this.next.as_ref() == Some(&next) {
                            cx.emit(PatchViewerEvent::Failed {
                                file_key: next.file_key.clone(),
                                message: error.to_string(),
                            });
                        }
                    });
                    return;
                }
            };
            // The same file, with lines shown around its changes: the new patch is filled in with
            // the whole file too, to show the same ones. Not an added or deleted file's, whose
            // patch has all its lines, and no other side to read.
            let Ok((same_file, keeping)) = this.update(cx, |this, cx| {
                let same_file = this
                    .shown
                    .as_ref()
                    .is_some_and(|shown| shown.patch.file_key == next.file_key);
                let whole = matches!(diff.kind, ChangeType::New | ChangeType::Deleted);
                let keeping = match &this.shown {
                    Some(shown) if same_file && !whole && !shown.diff.is_partial => Some((
                        Arc::clone(&shown.diff),
                        this.expanded.clone(),
                        load_files(Arc::clone(&diff), &this.load_file, cx),
                    )),
                    _ => None,
                };
                (same_file, keeping)
            }) else {
                return;
            };
            let (diff, expanded) = match keeping {
                Some((from, expanded, loading)) => match loading.await {
                    Ok(loaded) => {
                        let kept = carried_expansion(&from, &expanded, &loaded.hydrated);
                        (loaded.hydrated, kept)
                    }
                    // As the file changed again since, say.
                    Err(_) => (diff, Expanded::new()),
                },
                None => (diff, Expanded::new()),
            };
            let measured = Arc::clone(&diff);
            let (widest, highest) = cx.background_spawn(async move { measure(&measured) }).await;
            let Ok(waiting) = this.update(cx, |this, cx| {
                this.highlights.highlight(&diff, HIGHLIGHT_WAIT, cx)
            }) else {
                return;
            };
            waiting.await;
            let _ = this.update(cx, |this, cx| {
                // Passed over for another patch meanwhile.
                if this.next.as_ref() != Some(&next) {
                    return;
                }
                this.put_on_show(next, diff, expanded, same_file, (widest, highest), cx);
            });
        }));
    }

    fn put_on_show(
        &mut self,
        next: NextPatch,
        diff: Arc<FileDiff>,
        expanded: Expanded,
        same_file: bool,
        (widest, highest): (usize, usize),
        cx: &mut Context<Self>,
    ) {
        // Another file starts at its top.
        if self.shown.is_some() && !same_file {
            scroll_list_to(&self.scroll, px(0.), px(0.));
        }
        if same_file
            && let Some(shown) = &self.shown
            && shown.diff.cache_key != diff.cache_key
        {
            let replaced = Arc::clone(&shown.diff);
            self.highlights.forget(&replaced, cx);
        }
        let highlights = ViewerRuntime::highlight(&diff, cx);
        self.shown = Some(Shown {
            diff: Arc::clone(&diff),
            patch: next.clone(),
            highlights,
            widest,
            highest,
        });
        // The lines shown around the last one's changes are by hunk, which another's don't match.
        self.expanded = expanded;
        // Only the diff on show's failure matters: the others' would keep their diffs, and the
        // files read into them, for as long as the view is open (a file saved again and again).
        self.failures
            .retain(|(failed, _)| Arc::ptr_eq(failed, &diff));
        self.dragging = false;
        self.relayout();
        let pickable = self.is_pickable();
        if let Some(line) = self.picker.shown(same_file, pickable, &self.layout) {
            self.reveal(&line);
        }
        self.watch_highlights(cx);
        self.report(cx);
        cx.emit(PatchViewerEvent::Shown {
            file_key: next.file_key,
        });
        cx.notify();
    }

    /// Draws the diff on show again once its highlighting is done, if it isn't yet.
    fn watch_highlights(&mut self, cx: &mut Context<Self>) {
        let Some(shown) = &self.shown else {
            return;
        };
        if shown.highlights.peek().is_some() {
            self.highlighting = None;
            return;
        }
        let highlights = shown.highlights.clone();
        self.highlighting = Some(cx.spawn(async move |this, cx| {
            highlights.await;
            let _ = this.update(cx, |_, cx| cx.notify());
        }));
    }

    /// Lays the diff on show out again, with what may have changed.
    fn relayout(&mut self) {
        let Some(shown) = &self.shown else {
            self.layout = Rc::default();
            return;
        };
        let buttons = if self.is_pickable() {
            hunk_buttons(&shown.diff)
        } else {
            Vec::new()
        };
        // Without the buttons to show more, once the whole file couldn't be loaded.
        let failed = self
            .failures
            .iter()
            .any(|(diff, _)| Arc::ptr_eq(diff, &shown.diff));
        self.layout = Rc::new(layout(
            &shown.diff,
            LayoutOptions {
                split: self.style == DiffStyle::Split,
                expanded: &self.expanded,
                separators: if failed {
                    Separators::Simple
                } else {
                    Separators::LineInfo
                },
                can_load_files: true,
                hunk_buttons: &buttons,
            },
        ));
        // The lines picked are re-read from the rows, which side by side pair lines up.
        if let Some(selection) = self.picker.selection().copied() {
            self.picker.select(Some(selection), &self.layout);
        }
    }

    /// Says how loading the whole file on show is going, if that changed.
    fn report(&mut self, cx: &mut Context<Self>) {
        let (loading, error) = match &self.shown {
            Some(shown) => (
                self.loads.iter().any(|diff| Arc::ptr_eq(diff, &shown.diff)),
                self.failures
                    .iter()
                    .find(|(diff, _)| Arc::ptr_eq(diff, &shown.diff))
                    .map(|(_, message)| message.clone()),
            ),
            None => (false, None),
        };
        if self.reported == (loading, error.clone()) {
            return;
        }
        self.reported = (loading, error.clone());
        cx.emit(PatchViewerEvent::LoadingFiles(loading));
        cx.emit(PatchViewerEvent::FilesError(error));
    }

    /// Shows the unchanged lines hidden in the gap before the hunk `index`, `direction`-wise: of a
    /// partial diff, once the whole file is read (`loadDiffFiles`).
    fn expand_gap(&mut self, index: usize, direction: ExpandDirection, cx: &mut Context<Self>) {
        let Some(shown) = &self.shown else {
            return;
        };
        let diff = Arc::clone(&shown.diff);
        if !diff.is_partial {
            expand(&mut self.expanded, index, direction, gap_size(&diff, index));
            self.relayout();
            cx.notify();
            return;
        }
        if self
            .failures
            .iter()
            .any(|(failed, _)| Arc::ptr_eq(failed, &diff))
        {
            return;
        }
        self.loads.push(Arc::clone(&diff));
        self.report(cx);
        let loading = load_files(Arc::clone(&diff), &self.load_file, cx);
        cx.spawn(async move |this, cx| {
            let loaded = loading.await;
            // The whole file can have lines wider than the patch's, or more of them: measured off
            // the main thread, as it's read whole.
            let measured = match &loaded {
                Ok(loaded) => {
                    let hydrated = Arc::clone(&loaded.hydrated);
                    Some(cx.background_spawn(async move { measure(&hydrated) }).await)
                }
                Err(_) => None,
            };
            let _ = this.update(cx, |this, cx| {
                if let Some(ix) = this.loads.iter().position(|load| Arc::ptr_eq(load, &diff)) {
                    this.loads.remove(ix);
                }
                match loaded {
                    Ok(loaded) => {
                        let on_show = this
                            .shown
                            .as_ref()
                            .is_some_and(|shown| Arc::ptr_eq(&shown.diff, &diff));
                        if on_show {
                            this.highlights.remember(&loaded.hydrated);
                            let highlights = ViewerRuntime::highlight(&loaded.hydrated, cx);
                            if let Some(shown) = &mut this.shown {
                                shown.diff = Arc::clone(&loaded.hydrated);
                                shown.highlights = highlights;
                                if let Some((widest, highest)) = measured {
                                    shown.widest = widest;
                                    shown.highest = highest;
                                }
                            }
                            let range = gap_size(&loaded.hydrated, index);
                            expand(&mut this.expanded, index, direction, range);
                            this.relayout();
                            this.watch_highlights(cx);
                        }
                    }
                    Err(error) => {
                        this.failures.push((Arc::clone(&diff), error.to_string()));
                        this.relayout();
                    }
                }
                this.report(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Stages `changed`, of the patch on show, if lines can be staged now.
    fn stage(&mut self, changed: Vec<ChangedLine>, by_keyboard: bool, cx: &mut Context<Self>) {
        let Some(staging) = self.staging else {
            return;
        };
        if staging.busy || !self.is_live() {
            return;
        }
        let Some(patch) = self
            .shown
            .as_ref()
            .map(|shown| Arc::clone(&shown.patch.patch))
        else {
            return;
        };
        let Some(request) = self
            .picker
            .stage_request(changed, by_keyboard, staging.action)
        else {
            return;
        };
        self.staging_at = request.at;
        cx.emit(PatchViewerEvent::StageLines {
            patch,
            lines: to_selection(&request.lines),
        });
    }

    /// The lines asked for last ([`PatchViewerEvent::StageLines`]) couldn't be staged: the
    /// keyboard picks the same ones again, in the patch reloaded.
    pub fn stage_failed(&mut self) {
        let awaiting = self.is_awaiting_patch();
        self.picker.stage_failed(self.staging_at.take(), awaiting);
    }

    /// The rows in view.
    fn viewport(&self, row_height: Pixels) -> Viewport {
        let at = ListScroll::of(&self.scroll);
        Viewport {
            top: at.top / row_height,
            rows: at.height() / row_height,
        }
    }

    /// Scrolls the view, if it has to, to show `line`'s row, with a few rows around it in sight,
    /// as when moving through a list.
    fn reveal(&self, line: &ChangedLine) {
        let Some(row) = self.layout.row_of(line.line_number, line.side) else {
            return;
        };
        let at = ListScroll::of(&self.scroll);
        let Some(view) = at.view else {
            // Not laid out yet: from the top.
            self.scroll.scroll_to_item_with_offset(
                row,
                ScrollStrategy::Top,
                REVEAL_MARGIN_ROWS as usize,
            );
            return;
        };
        let row_height = self.row_height;
        let top = row_height * row as f32;
        let margin = row_height * REVEAL_MARGIN_ROWS;
        let target = if top - margin < at.top {
            Some((top - margin).max(px(0.)))
        } else if top + row_height + margin > at.top + view.height {
            Some(top + row_height + margin - view.height)
        } else {
            None
        };
        if let Some(top) = target {
            scroll_list_to(&self.scroll, at.left, top);
        }
    }

    /// Picks lines with `key` (see the viewer's actions, `viewer_actions`), if they can be picked
    /// now; whether it did.
    fn pick(&mut self, key: PickerKey, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let row_height = rems(ROW_REMS).to_pixels(window.rem_size());
        let viewport = self.viewport(row_height);
        let (pickable, live) = (self.is_pickable(), self.is_live());
        let layout = Rc::clone(&self.layout);
        match self.picker.key(key, pickable, live, &layout, viewport) {
            KeyOutcome::Ignored => return false,
            KeyOutcome::Handled { reveal } => {
                if let Some(line) = reveal {
                    self.reveal(&line);
                }
            }
            KeyOutcome::RevealHunk { first, last } => {
                self.reveal(&last);
                self.reveal(&first);
            }
            KeyOutcome::Stage => {
                let picked = self.picker.picked().to_vec();
                self.stage(picked, true, cx);
                return true;
            }
        }
        cx.notify();
        true
    }

    /// Picks lines with `key`; scrolls as `otherwise` says if they can't be picked.
    fn pick_or_scroll(
        &mut self,
        key: PickerKey,
        otherwise: Scroll,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.pick(key, window, cx) {
            self.scroll(otherwise, window, cx);
        }
    }

    /// Picks lines with `key`; leaves the key to the view around otherwise, e.g. Esc to close the
    /// changes.
    fn pick_or_propagate(&mut self, key: PickerKey, window: &mut Window, cx: &mut Context<Self>) {
        if !self.pick(key, window, cx) {
            cx.propagate();
        }
    }

    /// Scrolls, as a browser's scroll container does with the keyboard.
    fn scroll(&mut self, scroll: Scroll, window: &mut Window, cx: &mut Context<Self>) {
        let row_height = rems(ROW_REMS).to_pixels(window.rem_size());
        let page = self.viewport(row_height).rows.max(1.) * 0.9 * row_height;
        let line = row_height * 2.;
        let column = self.char_width.map_or(px(8.), |(_, width)| width) * 6.;
        let (down, right) = match scroll {
            Scroll::LineDown => (line, px(0.)),
            Scroll::LineUp => (-line, px(0.)),
            Scroll::Right => (px(0.), column),
            Scroll::Left => (px(0.), -column),
            Scroll::PageDown => (page, px(0.)),
            Scroll::PageUp => (-page, px(0.)),
            Scroll::Top => {
                self.scroll.scroll_to_item(0, ScrollStrategy::Top);
                cx.notify();
                return;
            }
            Scroll::Bottom => {
                self.scroll.scroll_to_bottom();
                cx.notify();
                return;
            }
        };
        scroll_list_by(
            &self.scroll,
            self.layout.rows.len(),
            down,
            right,
            row_height,
        );
        cx.notify();
    }

    /// Selects the line `number` of `side`, or extends the selection to it with Shift, starting a
    /// drag; clicking a line gives the view the keyboard, to go on from there.
    fn press_line(
        &mut self,
        number: usize,
        side: Side,
        extend: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        if !self.is_pickable() || !self.is_live() {
            return;
        }
        let range = match (extend, self.picker.selection()) {
            (true, Some(selected)) => SelectedRange {
                end: number,
                end_side: side,
                ..*selected
            },
            _ => SelectedRange::new(number, side, number),
        };
        self.picker.select(Some(range), &self.layout);
        self.dragging = true;
        cx.notify();
    }

    /// Extends a drag's selection to the line `number` of `side`.
    fn drag_line(&mut self, number: usize, side: Side, cx: &mut Context<Self>) {
        if !self.dragging {
            return;
        }
        let Some(selected) = self.picker.selection().copied() else {
            return;
        };
        if selected.end == number && selected.end_side == side {
            return;
        }
        let range = SelectedRange {
            end: number,
            end_side: side,
            ..selected
        };
        self.picker.select(Some(range), &self.layout);
        cx.notify();
    }

    fn end_drag(&mut self) {
        self.dragging = false;
    }

    /// The pointer is on the line `number` of `side`, or left it.
    fn hover_line(&mut self, number: usize, side: Side, hovered: bool, cx: &mut Context<Self>) {
        let line = self.layout.changed_line(number, side);
        let next = if hovered {
            line
        } else if self.picker.hovered() == line.as_ref() {
            None
        } else {
            return;
        };
        if self.picker.hover(next) {
            cx.notify();
        }
    }

    /// The width of a character of the code's font, at `font_size`.
    fn char_width(&mut self, font_size: Pixels, window: &mut Window, cx: &App) -> Pixels {
        if let Some((size, width)) = self.char_width
            && size == font_size
        {
            return width;
        }
        let width = code_char_width(font_size, window, cx);
        self.char_width = Some((font_size, width));
        width
    }

    /// What drawing the rows in view needs, taken once per frame.
    fn frame(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<Rc<Frame>> {
        let shown = self.shown.as_ref()?;
        let diff = Arc::clone(&shown.diff);
        let highlights = shown.highlights.peek().cloned();
        let (widest, highest) = (shown.widest, shown.highest);
        let rem = window.rem_size();
        let font_size = rems(FONT_REMS).to_pixels(rem);
        let row_height = rems(ROW_REMS).to_pixels(rem);
        self.row_height = row_height;
        let char_width = self.char_width(font_size, window, cx);
        let viewport_width = ListScroll::of(&self.scroll)
            .view
            .map_or(window.viewport_size().width, |view| view.width);
        let digits = highest.max(1).ilog10() as f32 + 1.;
        let gutter_width = char_width * digits + rem * 1.2;
        let code_padding = rem * 1.2;
        let text_width = char_width * widest as f32 + code_padding;
        let split = self.style == DiffStyle::Split;
        let column_width = if split {
            (gutter_width + text_width).max(viewport_width / 2.)
        } else {
            (gutter_width + text_width).max(viewport_width)
        };
        let row_width = if split {
            column_width * 2.
        } else {
            column_width
        };
        let layout = Rc::clone(&self.layout);
        let selected = self.picker.selection().and_then(|range| {
            let start = layout.row_of(range.start, range.side)?;
            let end = layout.row_of(range.end, range.end_side)?;
            Some((start.min(end), start.max(end)))
        });
        let to_stage = self.picker.to_stage();
        let pickable = self.is_pickable() && self.is_live();
        // The button in the gutter: by the last line selected, or else the changed line the
        // pointer is on. Only where lines can be picked: it takes the line number's place.
        let stage_button = to_stage
            .iter()
            .filter_map(|line| Some((layout.row_of(line.line_number, line.side)?, line.side)))
            .max_by_key(|(row, _)| *row)
            .filter(|_| pickable);
        Some(Rc::new(Frame {
            entity: cx.entity().downgrade(),
            layout,
            diff,
            highlights,
            split,
            row_width,
            column_width,
            gutter_width,
            row_height,
            font_size,
            selected,
            stage_button,
            staging: self
                .staging
                .as_ref()
                .map(|staging| (staging.action, staging.busy)),
            pickable,
            palette: *cx.palette(),
            mono: cx.theme().mono_font_family.clone(),
        }))
    }
}

/// The widest line of `diff`, in characters as drawn, and its highest line number.
fn measure(diff: &FileDiff) -> (usize, usize) {
    let widest = widest_line(diff.deletion_lines.iter().chain(diff.addition_lines.iter()));
    let mut highest = diff.deletion_lines.len().max(diff.addition_lines.len());
    if let Some(last) = diff.hunks.last() {
        highest = highest
            .max(last.addition_start + last.addition_count)
            .max(last.deletion_start + last.deletion_count);
    }
    (widest, highest)
}

/// What drawing the rows in view needs (see [`PatchViewer::frame`]).
struct Frame {
    entity: WeakEntity<PatchViewer>,
    layout: Rc<Layout>,
    diff: Arc<FileDiff>,
    highlights: Option<Arc<DiffHighlights>>,
    split: bool,
    row_width: Pixels,
    column_width: Pixels,
    gutter_width: Pixels,
    row_height: Pixels,
    font_size: Pixels,
    /// The rows selected, top and bottom.
    selected: Option<(usize, usize)>,
    /// Where the gutter's stage button is: its row and side.
    stage_button: Option<(usize, Side)>,
    /// What staging lines does, and whether it's busy.
    staging: Option<(StageAction, bool)>,
    /// Whether lines can be picked now.
    pickable: bool,
    palette: Palette,
    mono: SharedString,
}

impl Frame {
    fn tint(&self, side: Side) -> Hsla {
        match side {
            Side::Deletions => self.palette.coral,
            Side::Additions => self.palette.mint,
        }
    }

    fn is_selected(&self, row: usize) -> bool {
        self.selected
            .is_some_and(|(top, bottom)| row >= top && row <= bottom)
    }
}

impl Render for PatchViewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = *cx.palette();
        // Focused from the keyboard (`focus-visible`): not as a line's clicked.
        let focused = self.focus.is_focused(window) && window.last_input_was_keyboard();
        let frame = self.frame(window, cx);
        let busy = self.staging.as_ref().is_some_and(|staging| staging.busy);
        div()
            .id("patch-viewer")
            .key_context(PATCH_VIEWER)
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .min_h_0()
            .bg(palette.bg)
            .text_color(palette.text)
            .when(busy, |this| this.cursor_default())
            .on_action(cx.listener(|this, _: &SelectDown, window, cx| {
                this.pick_or_scroll(
                    PickerKey::Down { extend: false },
                    Scroll::LineDown,
                    window,
                    cx,
                )
            }))
            .on_action(cx.listener(|this, _: &SelectUp, window, cx| {
                this.pick_or_scroll(PickerKey::Up { extend: false }, Scroll::LineUp, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ExtendSelectionDown, window, cx| {
                this.pick_or_scroll(
                    PickerKey::Down { extend: true },
                    Scroll::LineDown,
                    window,
                    cx,
                )
            }))
            .on_action(cx.listener(|this, _: &ExtendSelectionUp, window, cx| {
                this.pick_or_scroll(PickerKey::Up { extend: true }, Scroll::LineUp, window, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectNextHunk, window, cx| {
                this.pick_or_propagate(PickerKey::NextHunk, window, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectPreviousHunk, window, cx| {
                this.pick_or_propagate(PickerKey::PreviousHunk, window, cx)
            }))
            .on_action(cx.listener(|this, _: &StageSelection, window, cx| {
                this.pick_or_scroll(PickerKey::Space, Scroll::PageDown, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ClearSelection, window, cx| {
                this.pick_or_propagate(PickerKey::Escape, window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &ScrollLeft, window, cx| {
                    this.scroll(Scroll::Left, window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &ScrollRight, window, cx| {
                this.scroll(Scroll::Right, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ScrollPageDown, window, cx| {
                this.scroll(Scroll::PageDown, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ScrollPageUp, window, cx| {
                this.scroll(Scroll::PageUp, window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &ScrollToTop, window, cx| {
                    this.scroll(Scroll::Top, window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &ScrollToBottom, window, cx| {
                this.scroll(Scroll::Bottom, window, cx)
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, window, cx| {
                    window.focus(&this.focus, cx);
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.end_drag()),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.end_drag()),
            )
            .when_some(frame, |this, frame| {
                let count = frame.layout.rows.len();
                let rows = Rc::clone(&frame);
                this.child(
                    uniform_list("patch-rows", count, move |range, window, cx| {
                        range
                            .map(|ix| render_row(&rows, ix, window, cx))
                            .collect::<Vec<_>>()
                    })
                    .track_scroll(&self.scroll)
                    .with_horizontal_sizing_behavior(ListHorizontalSizingBehavior::Unconstrained)
                    .size_full(),
                )
                .child(Scrollbar::new(&self.scroll))
            })
            // The focus ring, inside: the view fills its panel.
            .when(focused, |this| {
                this.child(
                    div()
                        .absolute()
                        .inset_0()
                        .border_1()
                        .border_color(palette.primary),
                )
            })
    }
}

/// The row `ix`.
fn render_row(frame: &Rc<Frame>, ix: usize, _: &mut Window, cx: &mut App) -> AnyElement {
    let row = frame.layout.rows[ix];
    let base = div()
        .id(("patch-row", ix))
        .flex_none()
        .w(frame.row_width)
        .min_w_full()
        .h(frame.row_height)
        .overflow_hidden();
    match row {
        Row::Line {
            kind,
            deletion,
            addition,
            ..
        } => {
            if frame.split {
                base.flex()
                    .child(split_cell(frame, ix, Side::Deletions, kind, deletion, cx))
                    .child(split_cell(frame, ix, Side::Additions, kind, addition, cx))
                    .into_any_element()
            } else {
                let (side, line) = match (addition, deletion) {
                    (Some(line), _) if kind != LineKind::Change || deletion.is_none() => {
                        (Side::Additions, line)
                    }
                    (_, Some(line)) => (Side::Deletions, line),
                    (Some(line), None) => (Side::Additions, line),
                    (None, None) => return base.into_any_element(),
                };
                base.child(line_cell(frame, ix, side, kind, line, frame.row_width, cx))
                    .into_any_element()
            }
        }
        Row::Separator {
            index,
            hidden,
            buttons,
        } => base
            .child(separator(frame, index, hidden, buttons))
            .into_any_element(),
        Row::Simple => base
            .flex()
            .items_center()
            .child(div().w_full().h(px(1.)).bg(frame.palette.border))
            .into_any_element(),
        Row::HunkButton { hunk } => base.child(hunk_button(frame, hunk)).into_any_element(),
        Row::NoNewline { deletion, addition } => {
            let note = |side: Side, shown: bool, width: Pixels| {
                div()
                    .w(width)
                    .h_full()
                    .flex()
                    .items_center()
                    .text_size(frame.font_size)
                    .text_color(frame.palette.faint)
                    .when(shown, |this| {
                        this.bg(frame.tint(side).opacity(0.06))
                            .child(div().flex_none().w(frame.gutter_width))
                            .child(div().pl(rems(0.6)).child("No newline at end of file"))
                    })
            };
            if frame.split {
                base.flex()
                    .child(note(Side::Deletions, deletion, frame.column_width))
                    .child(note(Side::Additions, addition, frame.column_width))
                    .into_any_element()
            } else {
                let side = if addition {
                    Side::Additions
                } else {
                    Side::Deletions
                };
                base.child(note(side, true, frame.row_width))
                    .into_any_element()
            }
        }
    }
}

/// One side of a row, side by side: its line, or an empty cell where the other side has a line
/// it doesn't.
fn split_cell(
    frame: &Rc<Frame>,
    ix: usize,
    side: Side,
    kind: LineKind,
    line: Option<LineAt>,
    cx: &mut App,
) -> AnyElement {
    let cell = match line {
        Some(line) => line_cell(frame, ix, side, kind, line, frame.column_width, cx),
        None => div()
            .w(frame.column_width)
            .h_full()
            .bg(frame.palette.panel.opacity(0.5))
            .into_any_element(),
    };
    div()
        .flex_none()
        .w(frame.column_width)
        .h_full()
        .when(side == Side::Deletions, |this| {
            this.border_r_1().border_color(frame.palette.border)
        })
        .child(cell)
        .into_any_element()
}

/// A line: its number, in the gutter, and its text, highlighted.
fn line_cell(
    frame: &Rc<Frame>,
    ix: usize,
    side: Side,
    kind: LineKind,
    line: LineAt,
    width: Pixels,
    cx: &mut App,
) -> AnyElement {
    let palette = &frame.palette;
    let changed = kind == LineKind::Change;
    let tint = frame.tint(side);
    let selected = frame.is_selected(ix);
    let number = line.number;
    let stage_here = frame.stage_button == Some((ix, side));
    let (text, highlights) = line_text(frame, side, line.index, changed, cx);
    let entity = frame.entity.clone();
    let hover = frame.entity.clone();
    let drag = frame.entity.clone();
    let pickable = frame.pickable;

    let gutter = div()
        .id(("gutter", ix * 2 + usize::from(side == Side::Additions)))
        .relative()
        .flex_none()
        .w(frame.gutter_width)
        .h_full()
        .flex()
        .items_center()
        .justify_end()
        .pr(rems(0.6))
        // In the code's font, which the gutter's width is measured in.
        .font_family(frame.mono.clone())
        .text_size(frame.font_size)
        .text_color(if changed { tint } else { palette.faint })
        .when(changed, |this| this.bg(tint.opacity(0.16)))
        .when(selected, |this| this.bg(palette.blue.opacity(0.3)))
        .when(pickable, |this| this.cursor_pointer())
        // The stage button takes the number's place.
        .when(!stage_here, |this| this.child(number.to_string()))
        // The change's bar, on the gutter's edge.
        .when(changed, |this| {
            this.child(
                div()
                    .absolute()
                    .left_0()
                    .top_0()
                    .bottom_0()
                    .w(rems(0.2))
                    .bg(tint),
            )
        })
        .when(stage_here, |this| this.child(stage_button(frame, ix)))
        .on_mouse_down(
            MouseButton::Left,
            move |event: &MouseDownEvent, window, cx| {
                let _ = entity.update(cx, |this, cx| {
                    this.press_line(number, side, event.modifiers.shift, window, cx)
                });
            },
        );

    div()
        .id(("line", ix * 2 + usize::from(side == Side::Additions)))
        .relative()
        .flex()
        .w(width)
        .min_w_full()
        .h_full()
        .when(changed, |this| this.bg(tint.opacity(0.1)))
        .when(selected, |this| this.bg(palette.blue.opacity(0.22)))
        .on_hover(move |hovered, _, cx| {
            let _ = hover.update(cx, |this, cx| this.hover_line(number, side, *hovered, cx));
        })
        .on_mouse_move(move |event: &MouseMoveEvent, _, cx| {
            if event.pressed_button == Some(MouseButton::Left) {
                let _ = drag.update(cx, |this, cx| this.drag_line(number, side, cx));
            }
        })
        .child(gutter)
        .child(
            div()
                .flex_none()
                .h_full()
                .flex()
                .items_center()
                .pl(rems(0.6))
                .whitespace_nowrap()
                .font_family(frame.mono.clone())
                .text_size(frame.font_size)
                .child(StyledText::new(text).with_highlights(highlights)),
        )
        .into_any_element()
}

/// The square button in the gutter that stages the lines selected, or the line the pointer is on.
fn stage_button(frame: &Rc<Frame>, ix: usize) -> AnyElement {
    let Some((action, busy)) = frame.staging else {
        return div().into_any_element();
    };
    let selected = frame.selected.is_some();
    let label = format!(
        "{} {}",
        if action == StageAction::Unstage {
            "Unstage"
        } else {
            "Stage"
        },
        if selected { "lines (Space)" } else { "line" }
    );
    let entity = frame.entity.clone();
    div()
        .id("stage-lines")
        .absolute()
        .left(rems(0.3))
        .top_0()
        .bottom_0()
        .flex()
        .items_center()
        // Not a press on the line it's on, which would select that line alone.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(
            Button::new(("stage-lines", ix))
                .primary()
                .xsmall()
                .icon(Icon::new(if action == StageAction::Unstage {
                    IconName::Minus
                } else {
                    IconName::Plus
                }))
                .accessibility_label(label.clone())
                .tooltip(label)
                .disabled(busy)
                .on_click(move |_, _, cx| {
                    let _ = entity.update(cx, |this, cx| {
                        let lines = this.picker.to_stage();
                        this.stage(lines, false, cx);
                    });
                }),
        )
        .into_any_element()
}

/// A hunk's button, above its first change.
fn hunk_button(frame: &Rc<Frame>, hunk: usize) -> AnyElement {
    let Some((action, busy)) = frame.staging else {
        return div().into_any_element();
    };
    let label = if action == StageAction::Unstage {
        "Unstage hunk"
    } else {
        "Stage hunk"
    };
    let entity = frame.entity.clone();
    h_flex()
        .w_full()
        .h_full()
        .justify_end()
        .px_2()
        .child(
            Button::new(("stage-hunk", hunk))
                .outline()
                .xsmall()
                .label(label)
                .disabled(busy)
                .on_click(move |_, _, cx| {
                    let _ = entity.update(cx, |this, cx| {
                        let lines = this.layout.changed().hunk_lines(hunk);
                        this.stage(lines, false, cx);
                    });
                }),
        )
        .into_any_element()
}

/// Where unchanged lines are hidden: how many, and the buttons that show them.
fn separator(
    frame: &Rc<Frame>,
    index: usize,
    hidden: Option<usize>,
    buttons: SeparatorButtons,
) -> AnyElement {
    let palette = &frame.palette;
    let text: SharedString = match hidden {
        Some(1) => "1 unmodified line".into(),
        Some(lines) => format!("{lines} unmodified lines").into(),
        None => "More unchanged context may be available".into(),
    };
    let button = |id: &'static str, icon: IconName, direction: ExpandDirection| {
        let entity = frame.entity.clone();
        Button::new((id, index))
            .ghost()
            .xsmall()
            .icon(Icon::new(icon))
            .accessibility_label("Show unmodified lines")
            .tooltip("Show unmodified lines")
            .on_click(move |_, _, cx| {
                let _ = entity.update(cx, |this, cx| this.expand_gap(index, direction, cx));
            })
    };
    let all = frame.entity.clone();
    h_flex()
        .h_full()
        .w_full()
        .px_2()
        .child(
            h_flex()
                .w_full()
                .h_full()
                .gap_1()
                .rounded_md()
                .bg(palette.panel_raised)
                .text_size(frame.font_size)
                .text_color(palette.muted)
                .when(buttons.from_start, |this| {
                    this.child(button(
                        "expand-down",
                        IconName::ChevronDown,
                        ExpandDirection::FromStart,
                    ))
                })
                .when(buttons.from_end, |this| {
                    this.child(button(
                        "expand-up",
                        IconName::ChevronUp,
                        ExpandDirection::FromEnd,
                    ))
                })
                .when(buttons.both, |this| {
                    this.child(button(
                        "expand-both",
                        IconName::ChevronsUpDown,
                        ExpandDirection::Both,
                    ))
                })
                .when(!buttons.any(), |this| this.pl_2())
                .child(div().pl_1().child(text))
                .when(buttons.all, |this| {
                    this.child(div().flex_1()).child(
                        Button::new(("expand-all", index))
                            .ghost()
                            .xsmall()
                            .label("Expand all")
                            .on_click(move |_, _, cx| {
                                let _ = all.update(cx, |this, cx| {
                                    this.expand_gap(index, ExpandDirection::All, cx)
                                });
                            }),
                    )
                }),
        )
        .into_any_element()
}

/// The text of line `ix` of `side` as it's drawn, and its highlights: tabs as spaces, a long
/// line cut, its syntax, and the words that changed.
fn line_text(
    frame: &Frame,
    side: Side,
    ix: usize,
    changed: bool,
    cx: &App,
) -> (SharedString, Vec<(Range<usize>, HighlightStyle)>) {
    let Some(line) = frame.diff.lines(side).get(ix) else {
        return (SharedString::default(), Vec::new());
    };
    let drawn = DrawnLine::new(line);
    let Some(highlights) = &frame.highlights else {
        return (drawn.text.into(), Vec::new());
    };
    let syntax = drawn.syntax(highlights, side, ix, cx);
    let words = if changed {
        highlights.changed(side, ix)
    } else {
        Default::default()
    };
    let words = words.iter().filter_map(|(range, ())| {
        let range = drawn.offset(range.start as usize)..drawn.offset(range.end as usize);
        (!range.is_empty()).then(|| {
            (
                range,
                HighlightStyle {
                    background_color: Some(frame.tint(side).opacity(0.3)),
                    ..HighlightStyle::default()
                },
            )
        })
    });
    let combined: Vec<_> = combine_highlights(syntax, words).collect();
    (drawn.text.into(), combined)
}

#[cfg(test)]
mod tests {
    use gpui_kit::{AppContext as _, TestAppContext};

    use super::super::viewer_runtime::LoadedFile;
    use super::*;
    use crate::data::QueryError;

    const PATCH: &str = "diff --git a/f.txt b/f.txt\nindex 1111111..2222222 100644\n--- a/f.txt\n+++ b/f.txt\n@@ -1,2 +1,2 @@\n one\n-two\n+TWO\n";

    /// Where the gutter's stage button is once the pointer is on the added line, with `staging`.
    fn stage_button_on_hover(staging: Option<LineStaging>, cx: &mut TestAppContext) -> bool {
        let _dir = crate::test_support::init(cx, |services| services);
        let load_file: LoadFile =
            Rc::new(|_, _, _| Task::ready(Err(QueryError::msg("Not read in tests."))));
        let (window, viewer) =
            crate::test_support::open(cx, |_, cx| PatchViewer::new(load_file, cx));
        cx.update(|cx| {
            viewer.update(cx, |viewer, cx| {
                viewer.set_props(
                    PatchProps {
                        patch: PATCH.into(),
                        file_key: "f.txt".into(),
                        cache_key: "test:f.txt".into(),
                        style: DiffStyle::Unified,
                        staging,
                    },
                    cx,
                )
            })
        });
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| {
            viewer.update(cx, |viewer, cx| {
                assert!(viewer.shown_diff().is_some());
                viewer.hover_line(2, Side::Additions, true, cx);
                viewer
                    .frame(window, cx)
                    .expect("a patch on show")
                    .stage_button
                    .is_some()
            })
        })
        .unwrap()
    }

    #[test]
    fn measures_lines_in_characters_as_drawn() {
        let patch = "diff --git a/f.txt b/f.txt\n--- a/f.txt\n+++ b/f.txt\n@@ -1 +1 @@\n-\u{e9}\u{e9}\u{e9}\n+\ta\n";
        let diff = gitto_ui::diff::diff_metadata::parse_patch_files(patch, None).remove(0);
        // Three characters of two bytes each, and a tab drawn as four spaces before a letter.
        assert_eq!(measure(&diff).0, TAB_WIDTH + 1);
        let long = format!(
            "diff --git a/f.txt b/f.txt\n--- a/f.txt\n+++ b/f.txt\n@@ -1 +1 @@\n-a\n+{}\n",
            "\u{e9}".repeat(MAX_DRAWN_LINE)
        );
        let diff = gitto_ui::diff::diff_metadata::parse_patch_files(&long, None).remove(0);
        // Cut at as many bytes, with an ellipsis.
        assert_eq!(measure(&diff).0, MAX_DRAWN_LINE / 2 + 1);
    }

    #[gpui_kit::test]
    fn offers_to_stage_the_changed_line_the_pointer_is_on(cx: &mut TestAppContext) {
        let staging = LineStaging {
            action: StageAction::Stage,
            busy: false,
        };
        assert!(stage_button_on_hover(Some(staging), cx));
    }

    #[gpui_kit::test]
    fn keeps_the_line_numbers_of_a_patch_whose_lines_cant_be_staged(cx: &mut TestAppContext) {
        // A commit's, say: the button would take the number's place, with nothing in it.
        assert!(!stage_button_on_hover(None, cx));
    }

    #[gpui_kit::test]
    fn widens_the_rows_for_a_line_wider_than_the_patchs_once_its_shown_around_it(
        cx: &mut TestAppContext,
    ) {
        let _dir = crate::test_support::init(cx, |services| services);
        // The first line, hidden above the hunk, is far wider than the patch's lines.
        let wide = "w".repeat(500);
        let old = format!("{wide}\ntwo\nthree\nfour\nfive\nsix\n");
        let new = format!("{wide}\ntwo\nthree\nfour\nfive\nSIX\n");
        let load_file: LoadFile = Rc::new(move |side, _, _| {
            let contents = match side {
                Side::Deletions => old.clone(),
                Side::Additions => new.clone(),
            };
            Task::ready(Ok(LoadedFile::Blob(contents)))
        });
        let (window, viewer) =
            crate::test_support::open(cx, |_, cx| PatchViewer::new(load_file, cx));
        cx.update(|cx| {
            viewer.update(cx, |viewer, cx| {
                viewer.set_props(
                    PatchProps {
                        patch: "diff --git a/f.txt b/f.txt\nindex 1111111..2222222 100644\n--- a/f.txt\n+++ b/f.txt\n@@ -5,2 +5,2 @@\n five\n-six\n+SIX\n".into(),
                        file_key: "f.txt".into(),
                        cache_key: "test:wide".into(),
                        style: DiffStyle::Unified,
                        staging: None,
                    },
                    cx,
                )
            })
        });
        cx.run_until_parked();
        let row_width = |cx: &mut TestAppContext| {
            cx.update_window(window, |_, window, cx| {
                viewer.update(cx, |viewer, cx| {
                    viewer.frame(window, cx).expect("a patch on show").row_width
                })
            })
            .unwrap()
        };
        let before = row_width(cx);
        viewer.update(cx, |viewer, cx| {
            viewer.expand_gap(0, ExpandDirection::All, cx)
        });
        cx.run_until_parked();
        assert!(viewer.read_with(cx, |viewer, _| !viewer.shown_diff().unwrap().is_partial));
        let after = row_width(cx);
        assert!(after > before * 2., "{before:?} to {after:?}");
    }
}
