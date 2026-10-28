//! The conflicts in a file: each one's sides, with buttons to keep ours, theirs or both, among the
//! lines around them. Ported from `conflict-viewer.tsx` and `conflict-actions.ts`, which drew with
//! @pierre/diffs' `UnresolvedFile`; this draws the rows `conflict_rows` lays out, in GPUI's
//! `uniform_list`, highlighted as the diff is (see `viewer_runtime`).

use std::collections::{HashSet, VecDeque};
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use futures::FutureExt as _;
use futures::future::Shared;
use gitto_ui::diff::diff_metadata::Side;
use gitto_ui::diff::patch_files::Expanded;
use gitto_ui::diff::viewer_rows::{ExpandDirection, SeparatorButtons, expand};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::scroll::Scrollbar;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _, h_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Context, EventEmitter, FocusHandle, Focusable, HighlightStyle, Hsla,
    KeyDownEvent, ListHorizontalSizingBehavior, MouseButton, Pixels, ScrollStrategy, SharedString,
    StyledText, Task, UniformListScrollHandle, WeakEntity, Window, div, point, px, rems,
    uniform_list,
};

pub use super::conflict_diff::Resolution;
use super::conflict_diff::{
    ConflictState, MarkerKind, ReadConflicts, conflicts_cache_key, conflicts_left, marker_label,
    read_conflicts, resolve,
};
use super::conflict_rows::{ConflictRow, LineRole, actions_row, conflict_rows};
use super::conflict_whole_file::WholeFile;
use super::file_editing::{EditSession, FileVersion};
use super::highlight::DiffHighlights;
use super::patch_viewer::{
    DrawnLine, FONT_REMS, ROW_REMS, code_char_width, scroll_list_by, widest_line,
};
use super::viewer_runtime::{HIGHLIGHT_WAIT, SharedHighlights, ViewHighlights, ViewerRuntime};
use crate::components::ui::{Icon, IconName};
use crate::data::QueryError;
use crate::theme::{Palette, PaletteExt as _};

/// How resolving the file on show is going.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ConflictProgress {
    /// Names the file on show (see `file_key`): another one's, until this one's is on show.
    pub file_key: Option<String>,
    /// How many conflicts are left.
    pub left: usize,
    /// Where the current one is among them, from 1; `None` without one.
    pub current: Option<usize>,
    /// Whether a conflict is being resolved: written to disk, and shown once it is.
    pub saving: bool,
    /// The version of the file on show on disk; `None` until it's known, e.g. after editing.
    pub version: Option<FileVersion>,
    /// Why the file's conflicts aren't shown one by one, as its whole text is instead.
    pub problem: Option<String>,
}

/// The file on disk, as last read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskFile {
    pub contents: String,
    pub version: FileVersion,
}

/// Writes a resolved conflict's text, `contents`, over the file at the version expected; resolves
/// to its new version.
pub type SaveResolved =
    Rc<dyn Fn(String, FileVersion, &mut App) -> Task<Result<FileVersion, QueryError>>>;

/// What the conflict viewer tells the file's view.
#[derive(Debug, Clone, PartialEq)]
pub enum ConflictViewerEvent {
    /// Editing the whole file started, or stopped (`onEditing`).
    Editing(Option<EditSession>),
    /// The whole file was edited: its text is read from its editor as it's saved.
    Edit,
    /// Why editing couldn't start.
    EditFailed(String),
    Progress(ConflictProgress),
    /// Why the last conflict couldn't be resolved; `None` once one is.
    Error(Option<String>),
    /// The file read last is on show.
    Shown,
}

/// The file's text and what's shown of it: its conflicts, or the whole of it without.
struct Shown {
    file_key: String,
    contents: Arc<str>,
    /// Its version on disk; `None` while that isn't known.
    version: Option<FileVersion>,
    /// Its conflicts, if they're shown one by one.
    state: Option<Arc<ConflictState>>,
    problem: Option<String>,
    /// The widest line, in characters as drawn.
    widest: usize,
}

/// A conflict to resolve, by its index in the text that was on show when it was asked.
struct Request {
    state: Arc<ConflictState>,
    index: usize,
    resolution: Resolution,
}

/// A conflicted file's conflicts, each with buttons to keep ours, theirs or both, and the lines
/// around them, more of which are shown when asked. Resolving one rewrites its region of the file,
/// which is written to disk (`save`) before it's shown, highlighted: in its place, so the view
/// stays where it is. A file without conflicts left, or whose markers can't be read as conflicts,
/// is shown whole instead (see `conflict_whole_file`).
///
/// Given the file as it's read from disk again ([`set_disk`](Self::set_disk)), it shows it only
/// if it's changed by something else since, e.g. another editor: not again as it was written
/// here, nor as it was read before that. While it's edited ([`set_editing`](Self::set_editing)),
/// the whole file is, markers and all, in place, and what's read from disk is held until editing
/// stops. The header's commands are its methods: [`next`](Self::next),
/// [`previous`](Self::previous) and [`resolve`](Self::resolve).
pub struct ConflictViewer {
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
    save: SaveResolved,
    path: SharedString,
    /// What the file's view last gave: the file, and the file as it was read from disk.
    given: Option<(String, Option<DiskFile>)>,
    shown: Option<Shown>,
    /// Versions of the file that were written over here: read from disk before that, and stale.
    superseded: HashSet<FileVersion>,
    /// The conflict the header's buttons and keys are of, by its index.
    current: Option<usize>,
    saving: bool,
    editing: bool,
    /// What's to be done, one thing at a time: the file as it was read again, and the conflicts
    /// the user resolved, in turn. Editing holds the rest until it stops.
    wanted: Option<(String, DiskFile)>,
    requests: VecDeque<Request>,
    running: bool,
    /// The work under way, which editing waits for.
    active: Shared<Task<()>>,
    pub(super) whole: WholeFile,
    /// The lines shown in each gap between the conflicts.
    expanded: Expanded,
    rows: Rc<Vec<ConflictRow>>,
    /// The diffs this view had highlighted, to drop the long ones once it closes.
    highlights: ViewHighlights,
    /// The highlighting of the conflicts on show.
    shown_highlights: Option<SharedHighlights>,
    highlighting: Option<Task<()>>,
    reported: Option<ConflictProgress>,
    /// The width of a character of the code's font, by the font size it's for.
    char_width: Option<(Pixels, Pixels)>,
}

impl EventEmitter<ConflictViewerEvent> for ConflictViewer {}

impl Focusable for ConflictViewer {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl ConflictViewer {
    pub fn new(save: SaveResolved, cx: &mut Context<Self>) -> Self {
        cx.on_release(|this: &mut Self, cx| this.highlights.drop_long(cx))
            .detach();
        ConflictViewer {
            // Focusable, to scroll it with the keyboard, and step through the conflicts with it.
            focus: cx.focus_handle().tab_stop(true),
            scroll: UniformListScrollHandle::new(),
            save,
            path: SharedString::default(),
            given: None,
            shown: None,
            superseded: HashSet::new(),
            current: None,
            saving: false,
            editing: false,
            wanted: None,
            requests: VecDeque::new(),
            running: false,
            active: Task::ready(()).shared(),
            whole: WholeFile::default(),
            expanded: Expanded::new(),
            rows: Rc::default(),
            highlights: ViewHighlights::default(),
            shown_highlights: None,
            highlighting: None,
            reported: None,
            char_width: None,
        }
    }

    /// The file `file_key` names, at `path`, as it was last read from disk; `None` until it's
    /// read. Another key is another file, which starts at its top.
    pub fn set_disk(
        &mut self,
        file_key: &str,
        path: &str,
        disk: Option<DiskFile>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let given = (file_key.to_owned(), disk);
        if self.given.as_ref() == Some(&given) {
            return;
        }
        self.path = SharedString::from(path.to_owned());
        self.wanted = given.1.clone().map(|disk| (given.0.clone(), disk));
        let run = self.wanted.is_some();
        self.given = Some(given);
        if run {
            drop(self.run(window, cx));
        }
    }

    /// Edits the whole file in place; turned off, the edits stay on show.
    pub fn set_editing(&mut self, editing: bool, window: &mut Window, cx: &mut Context<Self>) {
        if editing {
            self.start_editing(window, cx);
        } else {
            self.stop_editing(false, window, cx);
        }
    }

    /// Drops the edits made since the last save, and stops editing.
    pub fn discard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.stop_editing(true, window, cx);
    }

    /// Whether the whole file is being edited.
    pub fn is_editing(&self) -> bool {
        self.editing
    }

    /// Whether there's a selection in the editor for Escape to collapse.
    pub fn has_selection(&self, cx: &App) -> bool {
        self.whole.has_selection(cx)
    }

    /// The text on show, if any is.
    pub fn shown_text(&self) -> Option<&str> {
        self.shown.as_ref().map(|shown| &*shown.contents)
    }

    /// The rows on show, while the conflicts are.
    pub fn rows(&self) -> &[ConflictRow] {
        &self.rows
    }

    /// The whole file's editor, while it's on show.
    pub fn whole_file(&self) -> Option<&gpui_kit::Entity<gpui_kit::component::input::EditorState>> {
        self.whole.editor()
    }

    /// Whether a conflict can be resolved now: not while another's being saved, nor before the
    /// file's version on disk is known, which the write is checked against (after editing, say).
    fn is_resolvable(&self) -> bool {
        !self.saving
            && self
                .shown
                .as_ref()
                .is_some_and(|shown| shown.version.is_some())
    }

    fn state(&self) -> Option<&Arc<ConflictState>> {
        self.shown.as_ref().and_then(|shown| shown.state.as_ref())
    }

    /// Says how resolving the file is going, if that changed.
    fn report(&mut self, cx: &mut Context<Self>) {
        let left = conflicts_left(self.state().map(|state| &**state));
        let at = self
            .current
            .and_then(|current| left.iter().position(|ix| *ix == current));
        let progress = ConflictProgress {
            file_key: self.shown.as_ref().map(|shown| shown.file_key.clone()),
            left: left.len(),
            current: at.map(|at| at + 1),
            saving: self.saving,
            version: self.shown.as_ref().and_then(|shown| shown.version.clone()),
            problem: self.shown.as_ref().and_then(|shown| shown.problem.clone()),
        };
        cx.notify();
        if self.reported.as_ref() == Some(&progress) {
            return;
        }
        self.reported = Some(progress.clone());
        cx.emit(ConflictViewerEvent::Progress(progress));
    }

    /// Runs what's to be done, one thing at a time, unless it's under way; resolves once it's
    /// all done, or held by editing.
    fn run(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Shared<Task<()>> {
        if self.running {
            return self.active.clone();
        }
        self.running = true;
        let active = cx
            .spawn_in(window, async move |this, cx| {
                loop {
                    let Ok(Some(step)) =
                        this.update_in(cx, |this, window, cx| this.next_step(window, cx))
                    else {
                        break;
                    };
                    step.await;
                }
            })
            .shared();
        self.active = active.clone();
        active
    }

    /// What's to be done next: the file as it was read again, then the conflicts asked to be
    /// resolved. Nothing while it's edited.
    fn next_step(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<Task<()>> {
        if !self.editing {
            if let Some((file_key, disk)) = self.wanted.take() {
                return Some(self.sync(file_key, disk, window, cx));
            }
            if let Some(request) = self.requests.pop_front() {
                return Some(self.resolve_conflict(request, window, cx));
            }
        }
        self.running = false;
        None
    }

    /// Shows the file as it was read from disk, if that's news.
    fn sync(
        &mut self,
        file_key: String,
        disk: DiskFile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        let Some(shown) = &mut self.shown else {
            return self.show_text(
                file_key,
                disk.contents.into(),
                Some(disk.version),
                window,
                cx,
            );
        };
        if shown.file_key != file_key {
            self.superseded.clear();
            self.requests.clear();
            return self.show_text(
                file_key,
                disk.contents.into(),
                Some(disk.version),
                window,
                cx,
            );
        }
        if *shown.contents == *disk.contents {
            // Written here, or the same again: only its version is news.
            let known = shown.version.is_some();
            shown.version = Some(disk.version);
            self.report(cx);
            if !known {
                cx.emit(ConflictViewerEvent::Shown);
            }
            return Task::ready(());
        }
        if self.superseded.contains(&disk.version) {
            return Task::ready(());
        }
        // Changed elsewhere: what was asked of the last text isn't of this one.
        self.requests.clear();
        self.show_text(
            file_key,
            disk.contents.into(),
            Some(disk.version),
            window,
            cx,
        )
    }

    /// Shows `contents`: its conflicts, highlighted, or the whole of it. Another file starts at
    /// its top; the same one changed elsewhere stays where it's scrolled to.
    fn show_text(
        &mut self,
        file_key: String,
        contents: Arc<str>,
        version: Option<FileVersion>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        // Named by what's in it: a file that's back to what it was is shown as it was then.
        let cache_key = conflicts_cache_key(&file_key, &contents, version.as_deref());
        let path = self.path.to_string();
        cx.spawn_in(window, async move |this, cx| {
            let text = Arc::clone(&contents);
            let (read, widest) = cx
                .background_spawn(async move {
                    let read = read_conflicts(&path, &text, &cache_key);
                    let widest = read.state.as_ref().map_or(0, widest);
                    (read, widest)
                })
                .await;
            if let Some(state) = &read.state {
                let diff = Arc::clone(&state.diff);
                let Ok(waiting) = this.update(cx, |this, cx| {
                    this.highlights.highlight(&diff, HIGHLIGHT_WAIT, cx)
                }) else {
                    return;
                };
                waiting.await;
            }
            let _ = this.update_in(cx, |this, window, cx| {
                this.put_on_show(file_key, contents, version, read, widest, window, cx)
            });
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn put_on_show(
        &mut self,
        file_key: String,
        contents: Arc<str>,
        version: Option<FileVersion>,
        read: ReadConflicts,
        widest: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let other = self
            .shown
            .as_ref()
            .is_none_or(|shown| shown.file_key != file_key);
        let state = read.state.map(Arc::new);
        let last = self.state().map(|state| Arc::clone(&state.diff));
        match &state {
            Some(state) => {
                // The keyboard stays with the view, to step through the conflicts with it.
                let focused = self
                    .whole
                    .editor()
                    .is_some_and(|editor| editor.focus_handle(cx).is_focused(window));
                self.whole.hide();
                if focused {
                    window.focus(&self.focus, cx);
                }
                if other {
                    self.scroll
                        .0
                        .borrow()
                        .base_handle
                        .set_offset(point(px(0.), px(0.)));
                }
                // The lines shown around the conflicts are another file's.
                if other {
                    self.expanded = Expanded::new();
                }
                if let Some(last) = last
                    && last.cache_key != state.diff.cache_key
                {
                    self.highlights.forget(&last, cx);
                }
                self.watch_highlights(state, cx);
            }
            None => {
                let path = self.path.to_string();
                self.whole.show(&path, &contents, window, cx);
                self.shown_highlights = None;
            }
        }
        let has_version = version.is_some();
        self.current = conflicts_left(state.as_deref()).first().copied();
        self.shown = Some(Shown {
            file_key,
            contents,
            version,
            state,
            problem: read.problem,
            widest,
        });
        self.relayout();
        self.report(cx);
        // Only the file as it was read from disk: not edits whose version isn't known yet.
        if has_version {
            cx.emit(ConflictViewerEvent::Shown);
        }
    }

    /// Highlights `state`'s sides, drawing them again once that's done.
    fn watch_highlights(&mut self, state: &ConflictState, cx: &mut Context<Self>) {
        let highlights = ViewerRuntime::highlight(&state.diff, cx);
        self.shown_highlights = Some(highlights.clone());
        if highlights.peek().is_some() {
            self.highlighting = None;
            return;
        }
        self.highlighting = Some(cx.spawn(async move |this, cx| {
            highlights.await;
            let _ = this.update(cx, |_, cx| cx.notify());
        }));
    }

    fn relayout(&mut self) {
        self.rows = Rc::new(match self.state() {
            Some(state) => conflict_rows(state, &self.expanded),
            None => Vec::new(),
        });
    }

    /// Resolves the conflict `index` of the text on show, once what's asked before is done.
    fn request(
        &mut self,
        index: usize,
        resolution: Resolution,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.state() else {
            return;
        };
        if state.conflict(index).is_none() {
            return;
        }
        self.requests.push_back(Request {
            state: Arc::clone(state),
            index,
            resolution,
        });
        drop(self.run(window, cx));
    }

    /// Resolves the conflict asked for, if the text it was asked of is still the one on show:
    /// writes the text that leaves to disk, and shows it once it's written and highlighted, in the
    /// last one's place.
    fn resolve_conflict(
        &mut self,
        request: Request,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        let Some(from) = &self.shown else {
            return Task::ready(());
        };
        let (Some(state), Some(from_version)) = (&from.state, from.version.clone()) else {
            return Task::ready(());
        };
        if !Arc::ptr_eq(state, &request.state) {
            return Task::ready(());
        }
        let Some(resolved) = resolve(state, request.index, request.resolution).map(Arc::new) else {
            return Task::ready(());
        };
        let from_state = Arc::clone(state);
        self.saving = true;
        self.report(cx);
        let saving = (self.save)(resolved.contents().to_owned(), from_version.clone(), cx);
        let highlighting = self
            .highlights
            .highlight(&resolved.diff, HIGHLIGHT_WAIT, cx);
        let index = request.index;
        cx.spawn_in(window, async move |this, cx| {
            let (saved, ()) = futures::join!(saving, highlighting);
            let widest = widest(&resolved);
            let _ = this.update(cx, |this, cx| {
                match saved {
                    Ok(version) => {
                        this.superseded.insert(from_version);
                        if let Some(shown) = &mut this.shown {
                            shown.contents = Arc::from(resolved.contents());
                            shown.version = Some(version);
                            shown.state = Some(Arc::clone(&resolved));
                            shown.widest = widest;
                        }
                        // In the last one's place, the lines shown around the conflicts kept.
                        this.highlights.forget(&from_state.diff, cx);
                        this.watch_highlights(&resolved, cx);
                        this.relayout();
                        // On to the next one, or the one before if it was the last.
                        let left = conflicts_left(Some(&resolved));
                        this.current = left
                            .iter()
                            .copied()
                            .find(|other| *other > index)
                            .or_else(|| left.last().copied());
                        cx.emit(ConflictViewerEvent::Error(None));
                    }
                    Err(error) => {
                        // Not shown after all: its highlighting isn't kept either.
                        this.highlights.forget(&resolved.diff, cx);
                        cx.emit(ConflictViewerEvent::Error(Some(error.message())));
                    }
                }
                this.saving = false;
                this.report(cx);
            });
        })
    }

    /// Goes to the conflict after the current one, or before it, round to the other end.
    fn step(&mut self, forward: bool, cx: &mut Context<Self>) {
        let left = conflicts_left(self.state().map(|state| &**state));
        if left.is_empty() {
            return;
        }
        let at = self
            .current
            .and_then(|current| left.iter().position(|ix| *ix == current));
        let next = match at {
            None if forward => left[0],
            None => left[left.len() - 1],
            Some(at) if forward => left[(at + 1) % left.len()],
            Some(at) => left[(at + left.len() - 1) % left.len()],
        };
        self.current = Some(next);
        self.report(cx);
        self.scroll_to_conflict(next);
    }

    /// Scrolls the conflict `index`'s buttons to the middle of the view.
    fn scroll_to_conflict(&self, index: usize) {
        if let Some(row) = actions_row(&self.rows, index) {
            self.scroll.scroll_to_item(row, ScrollStrategy::Center);
        }
    }

    /// Scrolls to the next conflict left, which becomes the current one.
    pub fn next(&mut self, cx: &mut Context<Self>) {
        self.step(true, cx);
    }

    /// Scrolls to the previous conflict left, which becomes the current one.
    pub fn previous(&mut self, cx: &mut Context<Self>) {
        self.step(false, cx);
    }

    /// Resolves the current conflict.
    pub fn resolve(&mut self, resolution: Resolution, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(current) = self.current {
            self.request(current, resolution, window, cx);
        }
    }

    /// Edits the whole file, at the current conflict, once what's under way is done.
    fn start_editing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing {
            return;
        }
        self.editing = true;
        let active = self.active.clone();
        cx.spawn_in(window, async move |this, cx| {
            active.await;
            let _ = this.update_in(cx, |this, window, cx| {
                // Stopped while it waited: nothing edited yet.
                if !this.editing {
                    return;
                }
                let from = this
                    .shown
                    .as_ref()
                    .and_then(|shown| Some((shown.version.clone()?, Arc::clone(&shown.contents))));
                let Some((version, contents)) = from else {
                    this.editing = false;
                    cx.emit(ConflictViewerEvent::EditFailed(
                        "The file is still being saved. Try again in a moment.".to_owned(),
                    ));
                    drop(this.run(window, cx));
                    return;
                };
                let line = this
                    .current
                    .and_then(|current| this.state()?.conflict(current))
                    .map(|region| region.start_line);
                let path = this.path.to_string();
                this.whole.edit(&path, &contents, line, window, cx);
                let Some(editor) = this.whole.editor().cloned() else {
                    this.editing = false;
                    cx.emit(ConflictViewerEvent::EditFailed(
                        "This file can't be edited.".to_owned(),
                    ));
                    return;
                };
                cx.emit(ConflictViewerEvent::Editing(Some(EditSession {
                    version,
                    editor,
                })));
                cx.notify();
            });
        })
        .detach();
    }

    /// Stops editing: shows the edits' conflicts, or the file as it was before them if they're
    /// dropped, until it's read from disk again. Either way, its version on disk isn't known until
    /// then: the edits were saved meanwhile, or the file changed on disk, which is why they're
    /// dropped.
    fn stop_editing(&mut self, discard: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.editing {
            return;
        }
        self.editing = false;
        let text = self.whole.stop_editing(cx);
        if text.is_some() {
            cx.emit(ConflictViewerEvent::Editing(None));
        }
        let Some(shown) = &self.shown else {
            drop(self.run(window, cx));
            return;
        };
        let contents = match text {
            Some(text) if !discard => Arc::from(text),
            _ => Arc::clone(&shown.contents),
        };
        let showing = self.show_text(shown.file_key.clone(), contents, None, window, cx);
        cx.spawn_in(window, async move |this, cx| {
            showing.await;
            let _ = this.update_in(cx, |this, window, cx| {
                drop(this.run(window, cx));
            });
        })
        .detach();
        cx.notify();
    }

    /// Shows the lines hidden in the gap `gap`, `direction`-wise.
    fn expand_gap(
        &mut self,
        gap: usize,
        direction: ExpandDirection,
        range: usize,
        cx: &mut Context<Self>,
    ) {
        expand(&mut self.expanded, gap, direction, range);
        self.relayout();
        cx.notify();
    }

    /// Scrolls with the keyboard, as a browser's scroll container does.
    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let modifiers = &keystroke.modifiers;
        if self.whole.is_shown() || modifiers.alt || modifiers.control || modifiers.platform {
            return;
        }
        let row_height = rems(ROW_REMS).to_pixels(window.rem_size());
        let height = self
            .scroll
            .0
            .borrow()
            .last_item_size
            .map_or(px(0.), |size| size.item.height);
        let page = (height * 0.9).max(row_height);
        let line = row_height * 2.;
        let column = self.char_width.map_or(px(8.), |(_, width)| width) * 6.;
        let (down, right) = match keystroke.key.as_str() {
            "down" => (line, px(0.)),
            "up" => (-line, px(0.)),
            "right" => (px(0.), column),
            "left" => (px(0.), -column),
            "pagedown" => (page, px(0.)),
            "pageup" => (-page, px(0.)),
            "space" if modifiers.shift => (-page, px(0.)),
            "space" => (page, px(0.)),
            "home" => (px(-1e9), px(0.)),
            "end" => (px(1e9), px(0.)),
            _ => return,
        };
        scroll_list_by(&self.scroll, self.rows.len(), down, right, row_height);
        cx.stop_propagation();
        cx.notify();
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
        let state = Arc::clone(shown.state.as_ref()?);
        let widest = shown.widest;
        let rem = window.rem_size();
        let font_size = rems(FONT_REMS).to_pixels(rem);
        let row_height = rems(ROW_REMS).to_pixels(rem);
        let char_width = self.char_width(font_size, window, cx);
        let viewport_width = self
            .scroll
            .0
            .borrow()
            .last_item_size
            .map_or(window.viewport_size().width, |size| size.item.width);
        let highest = state.ours().len().max(state.theirs().len()).max(1);
        let digits = highest.ilog10() as f32 + 1.;
        let gutter_width = char_width * digits + rem * 1.2;
        let row_width = (gutter_width + char_width * widest as f32 + rem * 1.2).max(viewport_width);
        Some(Rc::new(Frame {
            entity: cx.entity().downgrade(),
            rows: Rc::clone(&self.rows),
            highlights: self
                .shown_highlights
                .as_ref()
                .and_then(|highlights| highlights.peek().cloned()),
            state,
            current: self.current,
            enabled: self.is_resolvable(),
            row_width,
            gutter_width,
            row_height,
            font_size,
            palette: *cx.palette(),
            mono: cx.theme().mono_font_family.clone(),
        }))
    }
}

/// Parses and highlights a conflicted file's conflicts ahead of showing them, as the viewer names
/// them, so they show at once (`prepareConflicts`). Nothing for one without conflicts, or whose
/// markers can't be read.
pub fn prepare_conflicts(
    file_key: &str,
    path: &str,
    contents: String,
    version: &str,
    cx: &mut App,
) {
    let key = conflicts_cache_key(file_key, &contents, Some(version));
    let path = path.to_owned();
    cx.spawn(async move |cx| {
        let state = cx
            .background_spawn(async move { read_conflicts(&path, &contents, &key).state })
            .await;
        if let Some(state) = state {
            cx.update(|cx| drop(ViewerRuntime::highlight(&state.diff, cx)));
        }
    })
    .detach();
}

/// The widest line of `state`'s sides, in characters as drawn.
fn widest(state: &ConflictState) -> usize {
    widest_line(state.ours().iter().chain(state.theirs().iter()))
}

/// What drawing the rows in view needs (see [`ConflictViewer::frame`]).
struct Frame {
    entity: WeakEntity<ConflictViewer>,
    rows: Rc<Vec<ConflictRow>>,
    state: Arc<ConflictState>,
    highlights: Option<Arc<DiffHighlights>>,
    current: Option<usize>,
    /// Whether the conflicts' buttons are on.
    enabled: bool,
    row_width: Pixels,
    gutter_width: Pixels,
    row_height: Pixels,
    font_size: Pixels,
    palette: Palette,
    mono: SharedString,
}

impl Frame {
    /// The colour of a side: ours green, theirs blue, as the TS viewer had them.
    fn tint(&self, role: LineRole) -> Option<Hsla> {
        match role {
            LineRole::Ours => Some(self.palette.mint),
            LineRole::Theirs => Some(self.palette.blue),
            _ => None,
        }
    }
}

impl Render for ConflictViewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = *cx.palette();
        let focused = self.focus.is_focused(window) && window.last_input_was_keyboard();
        let whole = self.whole.render();
        let frame = if whole.is_none() {
            self.frame(window, cx)
        } else {
            None
        };
        div()
            .id("conflicts")
            .key_context("ConflictViewer")
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .min_h_0()
            .bg(palette.bg)
            .text_color(palette.text)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    if !this.whole.is_shown() {
                        window.focus(&this.focus, cx);
                    }
                }),
            )
            .when_some(whole, |this, editor| this.child(editor))
            .when_some(frame, |this, frame| {
                let count = frame.rows.len();
                let rows = Rc::clone(&frame);
                this.child(
                    uniform_list("conflict-rows", count, move |range, _, cx| {
                        range
                            .map(|ix| render_row(&rows, ix, cx))
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
fn render_row(frame: &Rc<Frame>, ix: usize, cx: &App) -> AnyElement {
    let base = div()
        .id(("conflict-row", ix))
        .flex_none()
        .w(frame.row_width)
        .min_w_full()
        .h(frame.row_height)
        .overflow_hidden();
    match frame.rows[ix] {
        ConflictRow::Line {
            role,
            side,
            index,
            number,
            ..
        } => base
            .child(line_cell(frame, role, side, index, number, cx))
            .into_any_element(),
        ConflictRow::Actions { conflict } => {
            base.child(actions(frame, conflict)).into_any_element()
        }
        ConflictRow::Marker { kind, conflict } => {
            base.child(marker(frame, kind, conflict)).into_any_element()
        }
        ConflictRow::Separator {
            gap,
            hidden,
            range,
            buttons,
        } => base
            .child(separator(frame, gap, hidden, range, buttons))
            .into_any_element(),
    }
}

/// A line: its number, in the gutter, and its text, highlighted.
fn line_cell(
    frame: &Rc<Frame>,
    role: LineRole,
    side: Side,
    index: usize,
    number: usize,
    cx: &App,
) -> AnyElement {
    let palette = &frame.palette;
    let tint = frame.tint(role);
    let (text, highlights) = line_text(frame, side, index, cx);
    h_flex()
        .w_full()
        .h_full()
        .when_some(tint, |this, tint| this.bg(tint.opacity(0.12)))
        .child(
            div()
                .flex_none()
                .w(frame.gutter_width)
                .h_full()
                .flex()
                .items_center()
                .justify_end()
                .pr(rems(0.6))
                .font_family(frame.mono.clone())
                .text_size(frame.font_size)
                .text_color(tint.unwrap_or(palette.faint))
                .when_some(tint, |this, tint| this.bg(tint.opacity(0.16)))
                .child(number.to_string()),
        )
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

/// A conflict marker's row: the marker as it is in the file, its side named as the buttons name
/// it.
fn marker(frame: &Rc<Frame>, kind: MarkerKind, conflict: usize) -> AnyElement {
    let palette = &frame.palette;
    let line = frame
        .state
        .conflict(conflict)
        .and_then(|region| region.marker(kind))
        .unwrap_or_default()
        .trim_end_matches(['\n', '\r'])
        .to_owned();
    let (tint, side) = match kind {
        MarkerKind::Start => (Some(palette.mint), Some("(ours)")),
        MarkerKind::End => (Some(palette.blue), Some("(theirs)")),
        MarkerKind::Base | MarkerKind::Separator => (None, None),
    };
    h_flex()
        .w_full()
        .h_full()
        .when_some(tint, |this, tint| this.bg(tint.opacity(0.28)))
        .child(div().flex_none().w(frame.gutter_width).h_full())
        .child(
            h_flex()
                .flex_none()
                .h_full()
                .gap(rems(0.5))
                .pl(rems(0.6))
                .whitespace_nowrap()
                .font_family(frame.mono.clone())
                .text_size(frame.font_size)
                .child(line)
                .when_some(side, |this, side| {
                    this.child(div().text_color(palette.faint).child(side))
                }),
        )
        .into_any_element()
}

/// The buttons over a conflict: Keep ours, Keep theirs and Keep both, then the labels git gave its
/// sides, e.g. `HEAD ⟷ feature` (`renderConflictRow`). Marked as the current one, and turned off
/// while a conflict's being saved, or the file's version isn't known.
fn actions(frame: &Rc<Frame>, conflict: usize) -> AnyElement {
    let palette = &frame.palette;
    let current = frame.current == Some(conflict);
    let (ours, theirs) = frame
        .state
        .conflict(conflict)
        .map(|region| {
            (
                marker_label(&region.start_marker),
                marker_label(&region.end_marker),
            )
        })
        .unwrap_or_default();
    let choices = [
        (
            "keep-ours",
            Resolution::Ours,
            "Keep ours",
            if ours.is_empty() {
                "Keep ours".to_owned()
            } else {
                format!("Keep ours: {ours}")
            },
        ),
        (
            "keep-theirs",
            Resolution::Theirs,
            "Keep theirs",
            if theirs.is_empty() {
                "Keep theirs".to_owned()
            } else {
                format!("Keep theirs: {theirs}")
            },
        ),
        (
            "keep-both",
            Resolution::Both,
            "Keep both",
            "Keep both, ours first".to_owned(),
        ),
    ];
    let sides = if ours.is_empty() && theirs.is_empty() {
        String::new()
    } else {
        format!(
            "{} ⟷ {}",
            if ours.is_empty() { "ours" } else { &ours },
            if theirs.is_empty() { "theirs" } else { &theirs }
        )
    };
    h_flex()
        .id(("conflict-actions", conflict))
        .w_full()
        .h_full()
        .child(div().flex_none().w(frame.gutter_width).h_full())
        .child(
            h_flex()
                .min_w_0()
                .h_full()
                .gap_1()
                .pl_2()
                .text_xs()
                .text_color(if current {
                    palette.text_soft
                } else {
                    palette.muted
                })
                // The current conflict's mark, on its edge (`data-current`).
                .when(current, |this| {
                    this.border_l_2().border_color(palette.primary)
                })
                .children(choices.into_iter().map(|(id, resolution, label, title)| {
                    let entity = frame.entity.clone();
                    Button::new((id, conflict))
                        .outline()
                        .xsmall()
                        .label(label)
                        .tooltip(title)
                        .disabled(!frame.enabled)
                        .on_click(move |_, window, cx| {
                            let _ = entity.update(cx, |this, cx| {
                                // By its index, on the text on show when it's clicked.
                                this.request(conflict, resolution, window, cx)
                            });
                        })
                }))
                .when(!sides.is_empty(), |this| {
                    this.child(
                        div()
                            .ml_1p5()
                            .min_w_0()
                            .truncate()
                            .text_color(palette.faint)
                            .child(sides),
                    )
                }),
        )
        .into_any_element()
}

/// Where unchanged lines are hidden: how many, and the buttons that show them.
fn separator(
    frame: &Rc<Frame>,
    gap: usize,
    hidden: usize,
    range: usize,
    buttons: SeparatorButtons,
) -> AnyElement {
    let palette = &frame.palette;
    let text: SharedString = if hidden == 1 {
        "1 unmodified line".into()
    } else {
        format!("{hidden} unmodified lines").into()
    };
    let button = |id: &'static str, icon: IconName, direction: ExpandDirection| {
        let entity = frame.entity.clone();
        Button::new((id, gap))
            .ghost()
            .xsmall()
            .icon(Icon::new(icon))
            .accessibility_label("Show unmodified lines")
            .tooltip("Show unmodified lines")
            .on_click(move |_, _, cx| {
                let _ = entity.update(cx, |this, cx| this.expand_gap(gap, direction, range, cx));
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
                        Button::new(("expand-all", gap))
                            .ghost()
                            .xsmall()
                            .label("Expand all")
                            .on_click(move |_, _, cx| {
                                let _ = all.update(cx, |this, cx| {
                                    this.expand_gap(gap, ExpandDirection::All, range, cx)
                                });
                            }),
                    )
                }),
        )
        .into_any_element()
}

/// The text of line `ix` of `side` as it's drawn (see `DrawnLine`), and its syntax.
fn line_text(
    frame: &Frame,
    side: Side,
    ix: usize,
    cx: &App,
) -> (SharedString, Vec<(Range<usize>, HighlightStyle)>) {
    let lines = match side {
        Side::Deletions => frame.state.ours(),
        Side::Additions => frame.state.theirs(),
    };
    let Some(line) = lines.get(ix) else {
        return (SharedString::default(), Vec::new());
    };
    let drawn = DrawnLine::new(line);
    let syntax = match &frame.highlights {
        Some(highlights) => drawn.syntax(highlights, side, ix, cx).collect(),
        None => Vec::new(),
    };
    (drawn.text.into(), syntax)
}

#[cfg(test)]
mod tests {
    use gpui_kit::TestAppContext;

    use super::super::test_views::{
        Opened, conflicted, init, open, save, set_changes, set_conflict, text_conflict,
    };
    use super::*;
    use crate::git::diff_source::DiffSource;

    const MARKERS: &str = "<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> side\n";

    fn two_conflicts() -> String {
        format!("one\n{MARKERS}two\n{MARKERS}")
    }

    fn open_conflict(contents: &str, cx: &mut TestAppContext) -> Opened {
        set_changes(vec![], vec![conflicted("a.txt")], cx);
        set_conflict("a.txt", text_conflict(contents, "v1"), cx);
        open(DiffSource::Unstaged, conflicted("a.txt"), cx)
    }

    fn viewer(opened: &Opened, cx: &mut TestAppContext) -> gpui_kit::Entity<ConflictViewer> {
        opened.read(cx, |view, _| {
            view.resolution
                .viewer()
                .cloned()
                .expect("the conflicts' viewer")
        })
    }

    fn shown(opened: &Opened, cx: &mut TestAppContext) -> (Option<String>, Option<String>) {
        let viewer = viewer(opened, cx);
        cx.update(|cx| {
            let viewer = viewer.read(cx);
            (
                viewer.shown_text().map(str::to_owned),
                viewer
                    .reported
                    .as_ref()
                    .and_then(|p| p.version.as_deref().map(str::to_owned)),
            )
        })
    }

    #[gpui_kit::test]
    fn lays_out_each_conflict_with_its_buttons_and_markers(cx: &mut TestAppContext) {
        let _dir = init(cx);
        let opened = open_conflict(&two_conflicts(), cx);
        let viewer = viewer(&opened, cx);
        let rows = cx.update(|cx| viewer.read(cx).rows().to_vec());
        let actions = rows
            .iter()
            .filter(|row| matches!(row, ConflictRow::Actions { .. }))
            .count();
        assert_eq!(actions, 2);
        assert!(opened.has(("keep-both", 1usize), cx));
    }

    #[gpui_kit::test]
    fn takes_the_file_as_it_was_written_here_for_news_only_of_its_version(cx: &mut TestAppContext) {
        let _dir = init(cx);
        let opened = open_conflict(&two_conflicts(), cx);
        opened.click(("keep-ours", 0usize), cx);
        let written = format!("one\nours\ntwo\n{MARKERS}");
        assert_eq!(opened.tree.saves(), [save("a.txt", &written, "v1", false)]);
        assert_eq!(
            shown(&opened, cx),
            (Some(written.clone()), Some("v2".into()))
        );

        // Read again, as the watcher would once it's written.
        set_conflict("a.txt", text_conflict(&written, "v3"), cx);
        cx.run_until_parked();
        assert_eq!(shown(&opened, cx), (Some(written), Some("v3".into())));
    }

    #[gpui_kit::test]
    fn ignores_the_file_as_it_was_read_before_it_was_written_here(cx: &mut TestAppContext) {
        let _dir = init(cx);
        let opened = open_conflict(&two_conflicts(), cx);
        opened.click(("keep-theirs", 1usize), cx);
        let written = format!("one\n{MARKERS}two\ntheirs\n");

        // A read from before the write, coming in late.
        set_conflict("a.txt", text_conflict(&two_conflicts(), "v1"), cx);
        cx.run_until_parked();
        assert_eq!(shown(&opened, cx), (Some(written), Some("v2".into())));
    }

    #[gpui_kit::test]
    fn shows_the_file_changed_elsewhere_in_its_place(cx: &mut TestAppContext) {
        let _dir = init(cx);
        let opened = open_conflict(&two_conflicts(), cx);

        let elsewhere = format!("zero\n{MARKERS}");
        set_conflict("a.txt", text_conflict(&elsewhere, "v7"), cx);
        cx.run_until_parked();
        assert_eq!(shown(&opened, cx), (Some(elsewhere), Some("v7".into())));
    }

    #[gpui_kit::test]
    fn shows_a_file_whose_markers_cant_be_read_whole_and_says_why(cx: &mut TestAppContext) {
        let _dir = init(cx);
        let opened = open_conflict("a\n<<<<<<< HEAD\nours\n", cx);
        let viewer = viewer(&opened, cx);
        let (problem, whole) = cx.update(|cx| {
            let viewer = viewer.read(cx);
            (
                viewer.reported.as_ref().and_then(|p| p.problem.clone()),
                viewer.whole_file().is_some(),
            )
        });
        assert_eq!(
            problem.as_deref(),
            Some("This file's conflict markers can't be read as conflicts. Edit them by hand.")
        );
        assert!(whole);
    }
}
