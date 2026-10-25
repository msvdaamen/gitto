//! **Stub.** A file's changes. Replaces `components/repository/diff/file-diff-view.tsx`.

use gitto_git::features::diff::schema::ChangedFile;
use gitto_git::input::RepositoryId;
use gpui_kit::component::h_flex;
use gpui_kit::prelude::*;
use gpui_kit::{Context, EventEmitter, Task, Window, div};

use crate::components::placeholder::Placeholder;
use crate::components::ui::{IconButton, IconName};
use crate::git::diff_source::DiffSource;
use crate::theme::PaletteExt as _;

/// What a file's changes ask of the page.
#[derive(Debug, Clone, PartialEq)]
pub enum FileDiffEvent {
    /// Shows another file's changes, e.g. the next one in the list.
    Open {
        source: DiffSource,
        file: ChangedFile,
    },
    /// Goes back to the history.
    Close,
}

/// The changes in one file of a commit, a stash or the uncommitted changes, in place of the
/// history. Escape, or the back button, goes back to the history; the other files in the same
/// list are a step away. Uncommitted changes are kept up to date as they change on disk, in place;
/// once the file has none left on its side, that's said instead.
///
/// An uncommitted file can be staged, or unstaged, whole, or its lines a hunk or a selection at a
/// time, one at a time; an unstaged one can be edited. Once that leaves it without changes on its
/// side, the next file in its list opens. A conflicted one shows its conflicts instead, to resolve
/// (see `conflict_viewer`).
///
/// An entity, made for the file the page opens. The page asks it, before closing it or opening
/// another, whether it may ([`may_leave`](Self::may_leave)): its edits saved, or the user saying
/// what to do with them.
///
/// Stub: renders a placeholder, with the back button.
pub struct FileDiffView {
    repository_id: RepositoryId,
    source: DiffSource,
    file: ChangedFile,
}

impl EventEmitter<FileDiffEvent> for FileDiffView {}

impl FileDiffView {
    pub fn new(
        repository_id: RepositoryId,
        source: DiffSource,
        file: ChangedFile,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Self {
        FileDiffView {
            repository_id,
            source,
            file,
        }
    }

    pub fn repository_id(&self) -> &RepositoryId {
        &self.repository_id
    }

    pub fn source(&self) -> &DiffSource {
        &self.source
    }

    pub fn file(&self) -> &ChangedFile {
        &self.file
    }

    /// Resolves to whether the file may be closed, or another opened, once its edits are saved
    /// (TS's `onGuard`). Nothing to save in the stub.
    pub fn may_leave(&mut self, _: &mut Context<Self>) -> Task<bool> {
        Task::ready(true)
    }
}

impl Render for FileDiffView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.palette().bg)
            .child(
                h_flex().p_2().child(
                    IconButton::new("close-file", IconName::ArrowLeft, "Back to the history")
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(FileDiffEvent::Close))),
                ),
            )
            .child(
                div().flex_1().p_2().child(
                    Placeholder::new("File diff view", "diff/file-diff-view.tsx")
                        .detail(format!("{} ({:?})", self.file.path, self.source)),
                ),
            )
    }
}
