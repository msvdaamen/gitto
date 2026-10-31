//! **Stub.** A conflicted file's conflicts. Replaces
//! `components/repository/diff/conflict-viewer.tsx`.

use std::rc::Rc;

use gpui_kit::prelude::*;
use gpui_kit::{App, Context, EventEmitter, SharedString, Task, Window, div};

use crate::components::placeholder::Placeholder;
use crate::data::QueryError;

/// How a conflict is resolved: keeping our side, theirs, or both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    Ours,
    Theirs,
    Both,
}

/// How resolving the file on show is going.
#[derive(Debug, Clone, PartialEq)]
pub struct ConflictProgress {
    /// How many conflicts are left.
    pub left: usize,
    /// Where the current one is among them, from 1; `None` without one.
    pub current: Option<usize>,
    /// Whether a conflict is being resolved: written to disk, and shown once it is.
    pub saving: bool,
    /// The version of the file on show on disk; `None` until it's known, e.g. after editing.
    pub version: Option<String>,
    /// Why the file's conflicts aren't shown one by one, as its whole text is instead.
    pub problem: Option<String>,
}

/// The file on disk, as last read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskFile {
    pub contents: String,
    pub version: String,
}

/// Writes `contents` over the file at `version`; resolves to its new version.
pub type SaveFile = Rc<dyn Fn(String, String, &mut App) -> Task<Result<String, QueryError>>>;

/// What the conflict viewer tells the page.
#[derive(Debug, Clone, PartialEq)]
pub enum ConflictViewerEvent {
    /// Whether the whole file is being edited in place.
    Editing(bool),
    /// The whole file's text, as it's edited.
    Edit(String),
    EditFailed(String),
    Progress(ConflictProgress),
    /// Why the last conflict couldn't be resolved; `None` once one is.
    Error(Option<String>),
    /// The file read last is on show.
    Shown,
}

/// A conflicted file's conflicts, each with buttons to keep ours, theirs or both, and the lines
/// around them, more of which are shown when asked. Resolving one rewrites its region of the file,
/// which is written to disk (`save`) before it's shown, highlighted: in its place, so the view
/// stays where it is. A file without conflicts left, or whose markers can't be read as conflicts,
/// is shown whole instead.
///
/// Given the file as it's read from disk again ([`set_disk`](Self::set_disk)), it shows it only
/// if it's changed by something else since, e.g. another editor: not again as it was written
/// here, nor as it was read before that. While it's edited ([`set_editing`](Self::set_editing)),
/// the whole file is, markers and all, in place, and what's read from disk is held until editing
/// stops. The header's commands are its methods: [`next`](Self::next),
/// [`previous`](Self::previous) and [`resolve`](Self::resolve).
///
/// Stub: renders a placeholder.
pub struct ConflictViewer {
    /// Names the file: another name is another file, which starts at its top.
    file_key: SharedString,
    path: SharedString,
    disk: Option<DiskFile>,
    save: SaveFile,
    editing: bool,
}

impl EventEmitter<ConflictViewerEvent> for ConflictViewer {}

impl ConflictViewer {
    pub fn new(
        file_key: impl Into<SharedString>,
        path: impl Into<SharedString>,
        save: SaveFile,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Self {
        ConflictViewer {
            file_key: file_key.into(),
            path: path.into(),
            disk: None,
            save,
            editing: false,
        }
    }

    pub fn file_key(&self) -> &SharedString {
        &self.file_key
    }

    /// The file as it was last read from disk; `None` until it's read.
    pub fn set_disk(&mut self, disk: Option<DiskFile>, cx: &mut Context<Self>) {
        if self.disk != disk {
            self.disk = disk;
            cx.notify();
        }
    }

    /// Edits the whole file in place; turned off, the edits stay on show.
    pub fn set_editing(&mut self, editing: bool, cx: &mut Context<Self>) {
        if self.editing != editing {
            self.editing = editing;
            cx.notify();
        }
    }

    /// Scrolls to the next conflict left, which becomes the current one.
    pub fn next(&mut self, _: &mut Context<Self>) {}

    /// Scrolls to the previous conflict left, which becomes the current one.
    pub fn previous(&mut self, _: &mut Context<Self>) {}

    /// Resolves the current conflict.
    pub fn resolve(&mut self, _: Resolution, _: &mut Context<Self>) {
        let _ = &self.save;
    }
}

impl Render for ConflictViewer {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(
            Placeholder::new("Conflict viewer", "diff/conflict-viewer.tsx")
                .detail(self.path.clone()),
        )
    }
}
