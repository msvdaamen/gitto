//! Resolving the conflicted file on show, in its view. Ported from `conflict-resolution.tsx`.

use std::rc::Rc;

use gitto_git::FileStatus;
use gitto_git::features::conflicts::schema::{Conflict, Side};
use gitto_git::features::diff::schema::ChangedFile;
use gitto_git::input::RepositoryId;
use gitto_ui_logic::git::conflicts::{ConflictKind, conflict_kind, describe_conflict};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Disableable as _, Sizable as _};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context, Entity, FontWeight, KeyDownEvent, Task, Window, div};

use super::super::file_diff_view::{FileDiffEvent, FileDiffView};
use super::super::file_editing::Reload;
use super::conflict_sides::ConflictSides;
use super::conflict_viewer::{
    ConflictProgress, ConflictViewer, ConflictViewerEvent, DiskFile, Resolution, SaveFile,
};
use super::markers_dialog::open_markers_dialog;
use crate::components::ui::{
    EmptyState, IconButton, IconName, LoadingState, Notice, action_button,
};
use crate::data::conflicts::{ConflictResolution, FileToResolve, resolve_file_mutation};
use crate::data::keyed::Hold;
use crate::data::{DataError, Loaded, Mutation, Repositories, Repository};
use crate::theme::PaletteExt as _;
use gitto_ui_logic::git::diff_source::DiffSource;

/// Resolves a conflicted file whole: keeps a side, or marks it resolved.
pub type ResolveFileMutation = Entity<Mutation<FileToResolve>>;

/// The view's resolving of a conflicted file (`useConflictResolution`): conflict by conflict when
/// both sides changed it as text (see `ConflictViewer`), or by keeping one side whole, and then
/// marking it resolved, which stages it. Only once it has no conflict markers left; never by
/// itself. Once it's resolved, the next conflicted file opens, or this one's staged changes if it
/// was the last.
///
/// In the view, `[` and `]` step through the conflicts, `O`, `T` and `B` keep ours, theirs or both
/// of the current one, `N` opens the next conflicted file, and ⌘↵ marks the file resolved.
pub struct FileResolution {
    /// Holds the conflicted file while it's resolved, as it's loaded (see `held`).
    hold: Option<Hold>,
    /// What's held: the repository and the file's path, and whether it's held at all.
    held: (RepositoryId, String, bool),
    /// The conflicted file last read, by its path, as last synced: while another one's loads, the
    /// last one's stays on show, until it's in.
    conflict: Option<(String, Conflict)>,
    /// How many times the file on show has been read, and why it last couldn't be, as last
    /// synced.
    reads: u64,
    load_error: Option<DataError>,
    /// After editing, the file is read again before it's shown: what was read before is of the
    /// edits saved so far, not the last. The reads there were when editing stopped.
    read_after: Option<u64>,
    was_editing: bool,
    /// The viewer's progress, of the file it says.
    progress: Option<ConflictProgress>,
    /// Why the last thing done to the conflict failed, and of which file (its key).
    error: Option<(String, String)>,
    resolving: bool,
    resolve: ResolveFileMutation,
    viewer: Option<Entity<ConflictViewer>>,
}

impl FileResolution {
    pub fn new(
        repository_id: &RepositoryId,
        file: &ChangedFile,
        active: bool,
        cx: &mut Context<FileDiffView>,
    ) -> Self {
        let hold = active.then(|| {
            Repositories::open(repository_id, cx).update(cx, |repository, cx| {
                repository.hold_conflict(&file.path, cx)
            })
        });
        let resolve = resolve_file_mutation(cx);
        cx.observe(&resolve, |_, _, cx| cx.notify()).detach();
        FileResolution {
            hold,
            held: (repository_id.clone(), file.path.clone(), active),
            conflict: None,
            reads: 0,
            load_error: None,
            read_after: None,
            was_editing: false,
            progress: None,
            error: None,
            resolving: false,
            resolve,
            viewer: None,
        }
    }

    /// The conflicts' viewer, while the file's conflicts are shown one by one.
    pub fn viewer(&self) -> Option<&Entity<ConflictViewer>> {
        self.viewer.as_ref()
    }

    /// The conflicted file, as it was last read.
    pub fn conflict(&self) -> Option<Conflict> {
        self.conflict.as_ref().map(|(_, conflict)| conflict.clone())
    }

    fn kind(&self) -> Option<ConflictKind> {
        self.conflict
            .as_ref()
            .map(|(_, conflict)| conflict_kind(conflict))
    }

    /// Whether the conflict on show is the last file's, while this one's loads.
    fn is_placeholder(&self) -> bool {
        self.conflict
            .as_ref()
            .is_some_and(|(path, _)| *path != self.held.1)
    }

    /// Brings what it says up to date with what `repository` has read of the file at `path`.
    fn sync(&mut self, repository: &Repository, path: &str) {
        let loaded = repository.conflict(path);
        if let Some(conflict) = loaded.and_then(Loaded::data) {
            self.conflict = Some((path.to_owned(), conflict.clone()));
        }
        self.reads = loaded.map_or(0, Loaded::updates);
        self.load_error = loaded
            .and_then(Loaded::error)
            .filter(|_| self.held.2)
            .cloned();
    }

    fn is_text(&self) -> bool {
        matches!(self.kind(), Some(ConflictKind::Text { .. }))
    }
}

impl FileDiffView {
    /// Resolves files whole with `resolve` from now on: a test's.
    pub fn set_resolve_mutation(&mut self, resolve: ResolveFileMutation, cx: &mut Context<Self>) {
        cx.observe(&resolve, |_, _, cx| cx.notify()).detach();
        self.resolution.resolve = resolve;
    }

    /// Whether the file on show is conflicted, its conflicts shown to resolve: an unstaged one.
    pub(in crate::components::repository::diff) fn is_resolving(&self) -> bool {
        *self.view.source() == DiffSource::Unstaged && self.view.conflicted()
    }

    /// Brings the conflict up to date with the file on show: what's loaded, what the viewer
    /// shows, and whether it's edited.
    pub(in crate::components::repository::diff) fn sync_resolution(
        &mut self,
        show: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let active = self.is_resolving();
        let path = self.view.file().path;
        let repository = self.view.repository().clone();
        let held = (self.view.repository_id().clone(), path.clone(), active);
        if self.resolution.held != held {
            self.resolution.hold = active.then(|| {
                repository.update(cx, |repository, cx| repository.hold_conflict(&path, cx))
            });
            self.resolution.held = held;
        }
        self.resolution.sync(repository.read(cx), &path);
        // After editing, the file is read again before it's shown.
        let editing = self.editing.is_editing();
        if active && self.resolution.was_editing && !editing {
            self.resolution.read_after = Some(self.resolution.reads);
            repository.update(cx, |repository, cx| {
                drop(repository.reload_conflict(&path, cx))
            });
        }
        self.resolution.was_editing = editing;

        if !(show && active && self.resolution.is_text()) {
            if let Some(viewer) = self.resolution.viewer.take()
                && viewer.read(cx).is_editing()
            {
                // Taken off the page while it's edited: that's over, and what's left is saved.
                self.on_editing(None, cx);
            }
            return;
        }
        let viewer = match &self.resolution.viewer {
            Some(viewer) => viewer.clone(),
            None => self.make_conflict_viewer(cx),
        };
        let disk = self.disk();
        let key = self.view.file_key();
        viewer.update(cx, |viewer, cx| {
            viewer.set_disk(&key, &path, disk, window, cx);
            viewer.set_editing(editing, window, cx);
        });
    }

    fn make_conflict_viewer(&mut self, cx: &mut Context<Self>) -> Entity<ConflictViewer> {
        let view = cx.entity().downgrade();
        let save: SaveFile = Rc::new(move |contents, version, cx| {
            view.update(cx, |view, cx| view.save_conflict(contents, version, cx))
                .unwrap_or_else(|_| Task::ready(Err(DataError::msg("The file was closed."))))
        });
        let viewer = cx.new(|cx| ConflictViewer::new(save, cx));
        cx.subscribe(&viewer, |this, _, event, cx| {
            this.on_conflict_viewer(event, cx)
        })
        .detach();
        self.resolution.viewer = Some(viewer.clone());
        viewer
    }

    fn on_conflict_viewer(&mut self, event: &ConflictViewerEvent, cx: &mut Context<Self>) {
        match event {
            ConflictViewerEvent::Editing(session) => self.on_editing(session.clone(), cx),
            ConflictViewerEvent::Edit(text) => self.on_edit(text.clone(), cx),
            ConflictViewerEvent::EditFailed(message) => self.on_edit_failed(message.clone(), cx),
            ConflictViewerEvent::Progress(progress) => {
                self.resolution.progress = Some(progress.clone())
            }
            ConflictViewerEvent::Error(message) => {
                self.resolution.error = message
                    .clone()
                    .map(|message| (self.view.file_key(), message));
            }
            ConflictViewerEvent::Shown => self.on_shown_for_editing(cx),
        }
        cx.notify();
    }

    /// The file as it was read from disk, for the viewer: not while it's edited, nor the last
    /// file's, still on show while this one's loads, which the viewer would take for this one's;
    /// nor what was read before editing stopped.
    fn disk(&self) -> Option<DiskFile> {
        let resolution = &self.resolution;
        if self.editing.is_editing() || resolution.is_placeholder() {
            return None;
        }
        if let Some(after) = resolution.read_after
            && resolution.reads <= after
        {
            return None;
        }
        match resolution.kind()? {
            ConflictKind::Text { contents, version } => Some(DiskFile { contents, version }),
            _ => None,
        }
    }

    /// The viewer's progress, once it's of this file: the last one's is still on show while this
    /// one's conflicts are being highlighted.
    pub(in crate::components::repository::diff) fn shown_progress(
        &self,
    ) -> Option<&ConflictProgress> {
        let progress = self.resolution.progress.as_ref()?;
        (self.resolution.is_text()
            && progress.file_key.as_deref() == Some(self.view.file_key().as_str()))
        .then_some(progress)
    }

    /// Whether the file can't be resolved whole now: while it's being, nor while the last file's
    /// still on show, as this one's loads, nor before this one's version on disk is known, which a
    /// write is checked against (just after editing, say). Nor while it's edited: it's resolved
    /// as the edits leave it, which is only known once editing stops.
    fn is_resolution_busy(&self) -> bool {
        let resolution = &self.resolution;
        resolution.resolving
            || self.editing.is_editing()
            || resolution.is_placeholder()
            || (resolution.is_text()
                && self
                    .shown_progress()
                    .is_none_or(|shown| shown.saving || shown.version.is_none()))
    }

    /// The next conflicted file in the list after this one, or the first before it.
    pub(in crate::components::repository::diff) fn next_conflicted(&self) -> Option<ChangedFile> {
        let files = self.view.lists().files();
        let path = self.view.file().path;
        let at = files.iter().position(|file| file.path == path);
        let ordered: Vec<&ChangedFile> = match at {
            None => files.iter().collect(),
            Some(at) => files[at + 1..].iter().chain(&files[..at]).collect(),
        };
        ordered
            .into_iter()
            .find(|file| file.status == FileStatus::Conflicted && file.path != path)
            .cloned()
    }

    /// Whether it has no conflicts left in its text, so it's ready to be marked resolved. One
    /// that has is only marked resolved with them once the user says they belong in it.
    fn is_ready(&self) -> bool {
        self.shown_progress()
            .is_some_and(|shown| shown.left == 0 && shown.problem.is_none())
    }

    /// The version of the file to check before resolving it whole: the latest one written here.
    fn resolution_version(&self) -> Option<String> {
        self.shown_progress()
            .and_then(|shown| shown.version.clone())
            .or_else(|| {
                self.resolution
                    .conflict()
                    .and_then(|conflict| conflict.version)
            })
    }

    /// Resolves the file whole: marks it resolved, or keeps a side; then on to the next
    /// conflicted file, or to its staged changes if it was the last. Not while it's busy.
    fn resolve_whole(&mut self, resolution: ConflictResolution, cx: &mut Context<Self>) {
        if self.is_resolution_busy() {
            return;
        }
        let key = self.view.file_key();
        let file = self.view.file();
        self.resolution.resolving = true;
        // Picked while the file's still in its list: it goes once it's resolved.
        let target = self.next_conflicted();
        self.resolution.error = None;
        let input = FileToResolve {
            repository_id: self.view.repository_id().clone(),
            path: file.path.clone(),
            resolution,
        };
        let resolving = self
            .resolution
            .resolve
            .update(cx, |mutation, cx| mutation.mutate(input, cx));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = resolving.await;
            let _ = this.update(cx, |this, cx| {
                this.resolution.resolving = false;
                match result {
                    Ok(Ok(())) if key == this.view.file_key() => match target {
                        Some(target) => cx.emit(FileDiffEvent::Open {
                            source: DiffSource::Unstaged,
                            file: target,
                        }),
                        None => cx.emit(FileDiffEvent::Open {
                            source: DiffSource::Staged,
                            file: ChangedFile {
                                status: FileStatus::Modified,
                                ..file
                            },
                        }),
                    },
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => this.resolution.error = Some((key, error.message())),
                    Err(_) => {}
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Marks the file resolved, once it's ready; asks first if it has conflict markers left.
    pub(in crate::components::repository::diff) fn mark_resolved(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.resolution.is_text() || self.is_resolution_busy() {
            return;
        }
        let version = self.resolution_version();
        if self.is_ready() {
            self.resolve_whole(
                ConflictResolution::Mark {
                    version,
                    with_markers: false,
                },
                cx,
            );
            return;
        }
        let left = self.shown_progress().map_or(0, |shown| shown.left);
        let view = cx.entity().downgrade();
        open_markers_dialog(
            &self.view.file().path,
            left,
            move |_, cx| {
                let _ = view.update(cx, |this, cx| {
                    let version = this.resolution_version();
                    this.resolve_whole(
                        ConflictResolution::Mark {
                            version,
                            with_markers: true,
                        },
                        cx,
                    )
                });
            },
            window,
            cx,
        );
    }

    /// Keeps `side` of `conflict` whole.
    fn keep(&mut self, conflict: &Conflict, side: Side, cx: &mut Context<Self>) {
        let version = self.resolution_version();
        self.resolve_whole(
            ConflictResolution::Keep {
                side,
                sides: conflict.sides(),
                version,
            },
            cx,
        );
    }

    /// Writes a resolved conflict's text over the file at `version`; resolves to its new version.
    fn save_conflict(
        &mut self,
        contents: String,
        version: String,
        cx: &mut Context<Self>,
    ) -> Task<Result<String, DataError>> {
        let id = self.view.repository_id().clone();
        let path = self.view.file().path;
        let io = self.editing.io();
        let saving = (io.save)(&id, &path, contents, version, false, cx);
        cx.spawn(async move |_, cx| {
            let saved = saving.await;
            if saved.is_err() {
                // Changed on disk since, say: shown as it is now.
                cx.update(|cx| (io.reload)(&id, Reload::Conflict(path), cx));
            }
            saved
        })
    }

    /// The conflicts' keys: `[` and `]`, `O`, `T` and `B`, `N`, and ⌘↵ (or Ctrl+Enter). Not while
    /// the file's edited.
    pub(in crate::components::repository::diff) fn on_conflict_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.is_resolving() || self.editing.is_editing() {
            return false;
        }
        let keystroke = &event.keystroke;
        let modifiers = &keystroke.modifiers;
        let key = keystroke.key.as_str();
        if key == "enter" && (modifiers.platform || modifiers.control) {
            self.mark_resolved(window, cx);
            return true;
        }
        if modifiers.platform || modifiers.control || modifiers.alt {
            return false;
        }
        let resolution = match key {
            "o" => Some(Resolution::Ours),
            "t" => Some(Resolution::Theirs),
            "b" => Some(Resolution::Both),
            _ => None,
        };
        match key {
            "n" => {
                let Some(target) = self.next_conflicted() else {
                    return false;
                };
                cx.emit(FileDiffEvent::Open {
                    source: DiffSource::Unstaged,
                    file: target,
                });
                true
            }
            "]" | "[" => {
                if let Some(viewer) = &self.resolution.viewer {
                    viewer.update(cx, |viewer, cx| {
                        if key == "]" {
                            viewer.next(cx)
                        } else {
                            viewer.previous(cx)
                        }
                    });
                }
                true
            }
            _ => match resolution {
                Some(resolution) if !self.is_resolution_busy() && self.resolution.is_text() => {
                    if let Some(viewer) = &self.resolution.viewer {
                        viewer.update(cx, |viewer, cx| viewer.resolve(resolution, window, cx));
                    }
                    true
                }
                _ => false,
            },
        }
    }

    /// Whether the file can be edited by hand: its text, once it's on show as it is on disk.
    pub(in crate::components::repository::diff) fn is_conflict_editable(&self) -> bool {
        self.resolution.is_text()
            && self
                .shown_progress()
                .is_some_and(|shown| shown.version.is_some())
            && !self.resolution.is_placeholder()
    }

    /// The conflicts left, stepping through them, marking the file resolved, and the next file.
    pub(in crate::components::repository::diff) fn render_conflict_controls(
        &self,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let palette = *cx.palette();
        let editing = self.editing.is_editing();
        let progress = self.shown_progress();
        let left = progress
            .filter(|shown| shown.problem.is_none())
            .map(|shown| shown.left);
        let mut controls = Vec::new();
        if let Some(left) = left {
            let text = conflicts_left_label(left);
            controls.push(
                div()
                    .id("conflicts-left")
                    .role(gpui_kit::Role::Status)
                    .aria_label(text.clone())
                    .mr_1()
                    .whitespace_nowrap()
                    .text_size(gpui_kit::rems(0.733))
                    .text_color(palette.faint)
                    .child(text)
                    .into_any_element(),
            );
            controls.push(
                IconButton::new(
                    "previous-conflict",
                    IconName::ChevronUp,
                    "Previous conflict ([)",
                )
                .disabled(left == 0 || editing)
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(viewer) = &this.resolution.viewer {
                        viewer.update(cx, |viewer, cx| viewer.previous(cx));
                    }
                }))
                .into_any_element(),
            );
            controls.push(
                IconButton::new("next-conflict", IconName::ChevronDown, "Next conflict (])")
                    .disabled(left == 0 || editing)
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(viewer) = &this.resolution.viewer {
                            viewer.update(cx, |viewer, cx| viewer.next(cx));
                        }
                    }))
                    .into_any_element(),
            );
        }
        if self.resolution.is_text() {
            let ready = self.is_ready();
            controls.push(
                div()
                    .ml_1()
                    .child(
                        action_button("mark-resolved")
                            .when(ready, |button| button.primary())
                            .icon(crate::components::ui::Icon::new(IconName::Check))
                            .label("Mark resolved")
                            .disabled(self.is_resolution_busy())
                            .on_click(
                                cx.listener(|this, _, window, cx| this.mark_resolved(window, cx)),
                            ),
                    )
                    .into_any_element(),
            );
        }
        let next = self.next_conflicted();
        controls.push(
            IconButton::new(
                "next-conflicted-file",
                IconName::SkipForward,
                "Next conflicted file (N)",
            )
            .disabled(next.is_none())
            .on_click(cx.listener(move |_, _, _, cx| {
                if let Some(target) = next.clone() {
                    cx.emit(FileDiffEvent::Open {
                        source: DiffSource::Unstaged,
                        file: target,
                    });
                }
            }))
            .into_any_element(),
        );
        controls
    }

    /// What each side did with the file, and keeping one whole; or why that failed.
    pub(in crate::components::repository::diff) fn render_conflict_banner(
        &self,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut banners = Vec::new();
        let busy = self.is_resolution_busy();
        if self.resolution.is_text()
            && let Some(conflict) = self.resolution.conflict()
        {
            let message = self
                .shown_progress()
                .and_then(|shown| shown.problem.clone())
                .unwrap_or_else(|| describe_conflict(&conflict.sides()).to_owned());
            let (ours, theirs) = (conflict.clone(), conflict);
            banners.push(
                Notice::new(message)
                    .icon(IconName::GitMergeConflict)
                    .child(
                        Button::new("keep-all-ours")
                            .ghost()
                            .small()
                            .font_weight(FontWeight::SEMIBOLD)
                            .label("Keep all of ours")
                            .disabled(busy)
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.keep(&ours, Side::Ours, cx)),
                            ),
                    )
                    .child(
                        Button::new("keep-all-theirs")
                            .ghost()
                            .small()
                            .font_weight(FontWeight::SEMIBOLD)
                            .label("Keep all of theirs")
                            .disabled(busy)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.keep(&theirs, Side::Theirs, cx)
                            })),
                    )
                    .into_any_element(),
            );
        }
        if let Some((key, message)) = &self.resolution.error
            && *key == self.view.file_key()
        {
            banners.push(
                Notice::new(message.clone())
                    .alert()
                    .icon(IconName::TriangleAlert)
                    .into_any_element(),
            );
        }
        banners
    }

    /// The conflicts, or the sides to keep one of.
    pub(in crate::components::repository::diff) fn render_conflict_body(
        &self,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if let Some(error) = self.resolution.load_error.clone() {
            return EmptyState::new(
                "conflict-error",
                IconName::TriangleAlert,
                "Couldn't load the conflict",
            )
            .message(error.message())
            .error()
            .fill()
            .into_any_element();
        }
        let loading = || {
            LoadingState::new("conflict-loading", "Loading the conflict…")
                .fill()
                .into_any_element()
        };
        let Some(conflict) = self.resolution.conflict() else {
            return loading();
        };
        match conflict_kind(&conflict) {
            ConflictKind::Resolved => EmptyState::new(
                "conflict-resolved",
                IconName::FileCheck,
                "No longer conflicted",
            )
            .message("This file's conflict was resolved.")
            .fill()
            .into_any_element(),
            ConflictKind::Sides => {
                let busy = self.is_resolution_busy();
                let kept = conflict.clone();
                let view = cx.entity().downgrade();
                ConflictSides::new(conflict, busy, move |side, _, cx| {
                    let _ = view.update(cx, |this, cx| this.keep(&kept, side, cx));
                })
                .into_any_element()
            }
            // Loading until the viewer has the file on show, highlighted.
            ConflictKind::Text { .. } => match &self.resolution.viewer {
                Some(viewer) if viewer.read(cx).shown_text().is_some() => {
                    viewer.clone().into_any_element()
                }
                _ => loading(),
            },
        }
    }
}

/// What the header says of the conflicts left.
fn conflicts_left_label(left: usize) -> String {
    match left {
        0 => "No conflicts left".to_owned(),
        1 => "1 conflict left".to_owned(),
        left => format!("{left} conflicts left"),
    }
}

#[cfg(test)]
mod tests;
