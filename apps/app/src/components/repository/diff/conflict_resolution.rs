//! Resolving the conflicted file on show, in its view. Ported from `conflict-resolution.tsx`.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use gitto_git::FileStatus;
use gitto_git::features::conflicts::schema::{Conflict, Side};
use gitto_git::features::diff::schema::ChangedFile;
use gitto_git::input::RepositoryId;
use gitto_ui::git::conflicts::{ConflictKind, conflict_kind, describe_conflict};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Disableable as _, Sizable as _, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Context, Entity, EventEmitter, FontWeight, Subscription, Task, Window, div,
};

use super::conflict_sides::ConflictSides;
use super::conflict_viewer::{
    ConflictProgress, ConflictViewer, ConflictViewerEvent, DiskFile, Resolution, SaveResolved,
};
use super::file_diff_view::{FileDiffEvent, FileDiffView};
use super::file_editing::{EditSession, FileIo, FileVersion, Overwrite, SaveRequest};
use super::markers_dialog::open_markers_dialog;
use crate::components::ui::{
    EmptyState, IconButton, IconName, LoadingState, Notice, action_button,
};
use crate::data::conflicts::{
    ConflictResolution, FileToResolve, conflict_query, conflict_query_options,
    resolve_file_mutation,
};
use crate::data::{Mutation, Query, QueryError, keys};
use crate::theme::PaletteExt as _;
use gitto_ui::git::diff_source::DiffSource;

/// Resolves a conflicted file whole: keeps a side, or marks it resolved.
pub type ResolveFileMutation = Entity<Mutation<FileToResolve>>;

/// The file a resolution is of, as the file's view has it (see
/// [`ConflictResolutionView::set_file`]).
#[derive(Debug, Clone, PartialEq)]
pub struct ResolutionFile {
    pub repository_id: RepositoryId,
    /// The file on show, and its key (see `DiffSource::file_key`).
    pub file: ChangedFile,
    pub file_key: String,
    /// Whether it's resolved here: a conflicted file's unstaged changes.
    pub active: bool,
    /// Whether its conflicts are what the view shows.
    pub shown: bool,
    /// Whether it's being edited by hand (see `file_editing`).
    pub editing: bool,
    /// The next conflicted file in its list after it, or the first before it.
    pub next: Option<ChangedFile>,
}

/// What resolving the file tells the file's view.
#[derive(Debug, Clone, PartialEq)]
pub enum ConflictResolutionEvent {
    /// To open another file: the next conflicted one, or this one's staged changes once it's
    /// resolved.
    Open {
        source: DiffSource,
        file: ChangedFile,
    },
    /// Editing the whole file started, or stopped, in the conflicts' viewer (see `file_editing`).
    Editing(Option<EditSession>),
    /// The whole file was edited.
    Edit,
    /// Why editing couldn't start.
    EditFailed(String),
    /// The file read last is on show, as it is on disk.
    Shown,
}

/// The view's resolving of a conflicted file (`useConflictResolution`): conflict by conflict when
/// both sides changed it as text (see `ConflictViewer`), or by keeping one side whole, and then
/// marking it resolved, which stages it. Only once it has no conflict markers left; never by
/// itself. Once it's resolved, the next conflicted file opens, or this one's staged changes if it
/// was the last.
///
/// An entity of its own, which the file's view keeps and tells which file it's of
/// ([`set_file`](Self::set_file)); it loads the conflict itself, and tells the view what it should
/// do in turn through its events. Like the view, it brings what it shows up to date as it's
/// notified, so rendering it, or its header's controls and banners, only reads.
///
/// In the view, `[` and `]` step through the conflicts, `O`, `T` and `B` keep ours, theirs or both
/// of the current one, `N` opens the next conflicted file, and ⌘↵ marks the file resolved.
pub struct ConflictResolutionView {
    /// What the view said last of the file.
    file: ResolutionFile,
    query: Query<Conflict, Arc<Conflict>>,
    /// The text of the conflict loaded last, if it's resolved in its text: worked out once per
    /// load, rather than each time it's read, as it's the whole file.
    text: Option<(Arc<Conflict>, DiskFile)>,
    /// What the query is of: the file's path, and whether it's loaded at all.
    query_of: (String, bool),
    /// After editing, the file is read again before it's shown: what was read before is of the
    /// edits saved so far, not the last. The loads the query had when editing stopped.
    read_after: Option<u64>,
    was_editing: bool,
    /// The viewer's progress, of the file it says.
    progress: Option<ConflictProgress>,
    /// Why the last thing done to the conflict failed, and of which file (its key).
    error: Option<(String, String)>,
    resolving: bool,
    resolve: ResolveFileMutation,
    /// What the view reads and writes the working tree with (see `FileEditing::io`).
    io: Rc<RefCell<FileIo>>,
    viewer: Option<Entity<ConflictViewer>>,
    /// What the viewer tells it, for as long as it's kept.
    viewer_events: Option<Subscription>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ConflictResolutionEvent> for ConflictResolutionView {}

impl ConflictResolutionView {
    pub fn new(
        file: ResolutionFile,
        io: Rc<RefCell<FileIo>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let query = conflict_query(&file.repository_id, &file.file.path, file.active, cx);
        let resolve = resolve_file_mutation(cx);
        let this = cx.entity();
        let subscriptions = vec![
            cx.observe(&resolve, |_, _, cx| cx.notify()),
            // Notified, what it shows may have changed: it's synced before it renders again.
            cx.observe_in(&this, window, |this, _, window, cx| this.sync(window, cx)),
        ];
        let mut resolution = ConflictResolutionView {
            query_of: (file.file.path.clone(), file.active),
            file,
            query,
            text: None,
            read_after: None,
            was_editing: false,
            progress: None,
            error: None,
            resolving: false,
            resolve,
            io,
            viewer: None,
            viewer_events: None,
            _subscriptions: subscriptions,
        };
        resolution.sync(window, cx);
        resolution
    }

    /// Resolves files whole with `resolve` from now on: a test's.
    pub fn set_resolve_mutation(&mut self, resolve: ResolveFileMutation, cx: &mut Context<Self>) {
        self._subscriptions
            .push(cx.observe(&resolve, |_, _, cx| cx.notify()));
        self.resolve = resolve;
    }

    /// The file it's of, as the view has it now: synced with it as it's notified.
    pub fn set_file(&mut self, file: ResolutionFile, cx: &mut Context<Self>) {
        if self.file == file {
            return;
        }
        self.file = file;
        cx.notify();
    }

    /// The conflicts' viewer, while the file's conflicts are shown one by one.
    pub fn viewer(&self) -> Option<&Entity<ConflictViewer>> {
        self.viewer.as_ref()
    }

    /// The conflicted file, as it was last read.
    pub fn conflict(&self) -> Option<Arc<Conflict>> {
        self.query.data()
    }

    /// Why the last thing done to the conflict failed, if it's of the file on show.
    pub fn error(&self) -> Option<&str> {
        self.error
            .as_ref()
            .filter(|(key, _)| *key == self.file.file_key)
            .map(|(_, message)| message.as_str())
    }

    fn is_text(&self) -> bool {
        self.query
            .data()
            .is_some_and(|conflict| matches!(conflict_kind(&conflict), ConflictKind::Text { .. }))
    }

    /// Takes the text of the conflict loaded last, unless it has already.
    fn sync_text(&mut self) {
        let loaded = self.query.data();
        let taken = self.text.as_ref().map(|(conflict, _)| conflict);
        if loaded.as_ref().map(Arc::as_ptr) == taken.map(Arc::as_ptr) {
            return;
        }
        self.text = loaded.and_then(|conflict| {
            let ConflictKind::Text { contents, version } = conflict_kind(&conflict) else {
                return None;
            };
            let disk = DiskFile {
                version: version.into(),
                contents: contents.into(),
            };
            Some((conflict, disk))
        });
    }

    /// Brings the conflict up to date with the file: what's loaded, what the viewer shows, and
    /// whether it's edited. Run as it's notified, and as the file changes.
    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ResolutionFile {
            active,
            shown,
            editing,
            ..
        } = self.file;
        let path = self.file.file.path.clone();
        if self.query_of != (path.clone(), active) {
            let options = conflict_query_options(&self.file.repository_id, &path, active, cx);
            self.query.set_options(options, cx);
            self.query_of = (path.clone(), active);
        }
        // After editing, the file is read again before it's shown.
        if active && self.was_editing && !editing {
            self.read_after = Some(self.query.updates());
            let refetching = self.query.refetch(cx);
            cx.spawn(async move |_, _| refetching.await).detach();
        }
        self.was_editing = editing;
        self.sync_text();

        if !(shown && active && self.is_text()) {
            if let Some(viewer) = self.viewer.take() {
                self.viewer_events = None;
                if viewer.read(cx).is_editing() {
                    // Taken off the page while it's edited: that's over, and what's left is
                    // saved.
                    cx.emit(ConflictResolutionEvent::Editing(None));
                }
            }
            return;
        }
        let viewer = match &self.viewer {
            Some(viewer) => viewer.clone(),
            None => self.make_viewer(cx),
        };
        let disk = self.disk();
        let key = self.file.file_key.clone();
        viewer.update(cx, |viewer, cx| {
            viewer.set_disk(&key, &path, disk, window, cx);
            viewer.set_editing(editing, window, cx);
        });
    }

    fn make_viewer(&mut self, cx: &mut Context<Self>) -> Entity<ConflictViewer> {
        let resolution = cx.entity().downgrade();
        let save: SaveResolved = Rc::new(move |contents, version, cx| {
            resolution
                .update(cx, |resolution, cx| resolution.save(contents, version, cx))
                .unwrap_or_else(|_| Task::ready(Err(QueryError::msg("The file was closed."))))
        });
        let viewer = cx.new(|cx| ConflictViewer::new(save, cx));
        self.viewer_events =
            Some(cx.subscribe(&viewer, |this, _, event, cx| this.on_viewer(event, cx)));
        self.viewer = Some(viewer.clone());
        viewer
    }

    pub(super) fn on_viewer(&mut self, event: &ConflictViewerEvent, cx: &mut Context<Self>) {
        match event {
            ConflictViewerEvent::Editing(session) => {
                cx.emit(ConflictResolutionEvent::Editing(session.clone()))
            }
            ConflictViewerEvent::Edit => cx.emit(ConflictResolutionEvent::Edit),
            ConflictViewerEvent::EditFailed(message) => {
                cx.emit(ConflictResolutionEvent::EditFailed(message.clone()))
            }
            ConflictViewerEvent::Progress(progress) => self.progress = Some(progress.clone()),
            ConflictViewerEvent::Error(message) => {
                self.error = message
                    .clone()
                    .map(|message| (self.file.file_key.clone(), message));
            }
            ConflictViewerEvent::Shown => cx.emit(ConflictResolutionEvent::Shown),
        }
        cx.notify();
    }

    /// The file as it was read from disk, for the viewer: not while it's edited, nor the last
    /// file's, still on show while this one's loads, which the viewer would take for this one's;
    /// nor what was read before editing stopped.
    pub(super) fn disk(&self) -> Option<DiskFile> {
        if self.file.editing || self.query.is_placeholder_data() {
            return None;
        }
        if let Some(after) = self.read_after
            && self.query.updates() <= after
        {
            return None;
        }
        self.text.as_ref().map(|(_, disk)| disk.clone())
    }

    /// The viewer's progress, once it's of this file: the last one's is still on show while this
    /// one's conflicts are being highlighted.
    pub fn shown_progress(&self) -> Option<&ConflictProgress> {
        let progress = self.progress.as_ref()?;
        (self.is_text() && progress.file_key.as_deref() == Some(self.file.file_key.as_str()))
            .then_some(progress)
    }

    /// Whether the file can't be resolved whole now: while it's being, nor while the last file's
    /// still on show, as this one's loads, nor before this one's version on disk is known, which a
    /// write is checked against (just after editing, say). Nor while it's edited: it's resolved
    /// as the edits leave it, which is only known once editing stops.
    fn is_busy(&self) -> bool {
        self.resolving
            || self.file.editing
            || self.query.is_placeholder_data()
            || (self.is_text()
                && self
                    .shown_progress()
                    .is_none_or(|shown| shown.saving || shown.version.is_none()))
    }

    /// Whether it has no conflicts left in its text, so it's ready to be marked resolved. One
    /// that has is only marked resolved with them once the user says they belong in it.
    fn is_ready(&self) -> bool {
        self.shown_progress()
            .is_some_and(|shown| shown.left == 0 && shown.problem.is_none())
    }

    /// The version of the file to check before resolving it whole: the latest one written here.
    fn version(&self) -> Option<String> {
        self.shown_progress()
            .and_then(|shown| shown.version.clone().map(FileVersion::into_string))
            .or_else(|| {
                self.conflict()
                    .and_then(|conflict| conflict.version.clone())
            })
    }

    /// Resolves the file whole: marks it resolved, or keeps a side; then on to the next
    /// conflicted file, or to its staged changes if it was the last. Not while it's busy.
    fn resolve_whole(&mut self, resolution: ConflictResolution, cx: &mut Context<Self>) {
        if self.is_busy() {
            return;
        }
        let key = self.file.file_key.clone();
        let file = self.file.file.clone();
        self.resolving = true;
        // Picked while the file's still in its list: it goes once it's resolved.
        let target = self.file.next.clone();
        self.error = None;
        let input = FileToResolve {
            repository_id: self.file.repository_id.clone(),
            path: file.path.clone(),
            resolution,
        };
        let resolving = self
            .resolve
            .update(cx, |mutation, cx| mutation.mutate(input, cx));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = resolving.await;
            let _ = this.update(cx, |this, cx| {
                this.resolving = false;
                match result {
                    Ok(Ok(())) if key == this.file.file_key => match target {
                        Some(target) => cx.emit(ConflictResolutionEvent::Open {
                            source: DiffSource::Unstaged,
                            file: target,
                        }),
                        None => cx.emit(ConflictResolutionEvent::Open {
                            source: DiffSource::Staged,
                            file: ChangedFile {
                                status: FileStatus::Modified,
                                ..file
                            },
                        }),
                    },
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => this.error = Some((key, error.message())),
                    Err(_) => {}
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Marks the file resolved, once it's ready; asks first if it has conflict markers left.
    /// Whether that was for this file's resolution: its key, say, even while it's busy.
    pub fn mark_resolved(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.is_text() || self.is_busy() {
            return true;
        }
        let version = self.version();
        if self.is_ready() {
            self.resolve_whole(
                ConflictResolution::Mark {
                    version,
                    with_markers: false,
                },
                cx,
            );
            return true;
        }
        let left = self.shown_progress().map_or(0, |shown| shown.left);
        let resolution = cx.entity().downgrade();
        open_markers_dialog(
            &self.file.file.path,
            left,
            move |_, cx| {
                let _ = resolution.update(cx, |this, cx| {
                    let version = this.version();
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
        true
    }

    /// Keeps `side` of `conflict` whole.
    fn keep(&mut self, conflict: &Conflict, side: Side, cx: &mut Context<Self>) {
        let version = self.version();
        self.resolve_whole(
            ConflictResolution::Keep {
                side,
                sides: conflict.sides(),
                version,
            },
            cx,
        );
    }

    /// Writes a resolved conflict's text over the file at the version `expected`; resolves to
    /// its new version.
    fn save(
        &mut self,
        contents: String,
        expected: FileVersion,
        cx: &mut Context<Self>,
    ) -> Task<Result<FileVersion, QueryError>> {
        let id = self.file.repository_id.clone();
        let path = self.file.file.path.clone();
        let io = self.io.borrow().clone();
        let request = SaveRequest {
            contents,
            expected,
            overwrite: Overwrite::IfUnchanged,
        };
        let saving = (io.save)(&id, &path, request, cx);
        cx.spawn(async move |_, cx| {
            let saved = saving.await;
            if saved.is_err() {
                // Changed on disk since, say: shown as it is now.
                cx.update(|cx| (io.reload)(keys::conflict(&id, &path), cx));
            }
            saved
        })
    }

    /// Opens the next conflicted file, if there's one; whether there was.
    pub fn open_next(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(target) = self.file.next.clone() else {
            return false;
        };
        cx.emit(ConflictResolutionEvent::Open {
            source: DiffSource::Unstaged,
            file: target,
        });
        true
    }

    /// Steps to the next conflict, or the one before.
    pub fn step(&mut self, forward: bool, cx: &mut Context<Self>) -> bool {
        if let Some(viewer) = &self.viewer {
            viewer.update(cx, |viewer, cx| {
                if forward {
                    viewer.next(cx)
                } else {
                    viewer.previous(cx)
                }
            });
        }
        true
    }

    /// Whether the conflicts' keys are its own now (see `viewer_actions`): `[` and `]`, `O`, `T`
    /// and `B`, `N`, and ⌘↵ (or Ctrl+Enter). While the file's resolved here, but not while it's
    /// edited by hand, when they're the editor's.
    pub fn takes_keys(&self) -> bool {
        self.file.active && !self.file.editing
    }

    /// Keeps `resolution` of the current conflict, if it can be kept now; whether it could.
    pub fn keep_current(
        &mut self,
        resolution: Resolution,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.is_busy() || !self.is_text() {
            return false;
        }
        if let Some(viewer) = &self.viewer {
            viewer.update(cx, |viewer, cx| viewer.resolve(resolution, window, cx));
        }
        true
    }

    /// Whether the file can be edited by hand: its text, once it's on show as it is on disk.
    pub fn is_editable(&self) -> bool {
        self.is_text()
            && self
                .shown_progress()
                .is_some_and(|shown| shown.version.is_some())
            && !self.query.is_placeholder_data()
    }

    /// The header's controls, of the resolution `this`: the conflicts left, stepping through
    /// them, marking the file resolved, and the next file.
    pub fn render_controls(&self, this: &Entity<Self>, cx: &App) -> Vec<AnyElement> {
        let palette = *cx.palette();
        let editing = self.file.editing;
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
            let previous = this.clone();
            controls.push(
                IconButton::new(
                    "previous-conflict",
                    IconName::ChevronUp,
                    "Previous conflict ([)",
                )
                .disabled(left == 0 || editing)
                .on_click(move |_, _, cx| {
                    previous.update(cx, |this, cx| this.step(false, cx));
                })
                .into_any_element(),
            );
            let next = this.clone();
            controls.push(
                IconButton::new("next-conflict", IconName::ChevronDown, "Next conflict (])")
                    .disabled(left == 0 || editing)
                    .on_click(move |_, _, cx| {
                        next.update(cx, |this, cx| this.step(true, cx));
                    })
                    .into_any_element(),
            );
        }
        if self.is_text() {
            let ready = self.is_ready();
            let mark = this.clone();
            controls.push(
                div()
                    .ml_1()
                    .child(
                        action_button("mark-resolved")
                            .when(ready, |button| button.primary())
                            .icon(crate::components::ui::Icon::new(IconName::Check))
                            .label("Mark resolved")
                            .disabled(self.is_busy())
                            .on_click(move |_, window, cx| {
                                mark.update(cx, |this, cx| this.mark_resolved(window, cx));
                            }),
                    )
                    .into_any_element(),
            );
        }
        let open = this.clone();
        controls.push(
            IconButton::new(
                "next-conflicted-file",
                IconName::SkipForward,
                "Next conflicted file (N)",
            )
            .disabled(self.file.next.is_none())
            .on_click(move |_, _, cx| {
                open.update(cx, |this, cx| {
                    this.open_next(cx);
                })
            })
            .into_any_element(),
        );
        controls
    }

    /// The header's banners, of the resolution `this`: what each side did with the file, and
    /// keeping one whole; or why that failed.
    pub fn render_banners(&self, this: &Entity<Self>) -> Vec<AnyElement> {
        let mut banners = Vec::new();
        let busy = self.is_busy();
        if self.is_text()
            && let Some(conflict) = self.conflict()
        {
            let message = self
                .shown_progress()
                .and_then(|shown| shown.problem.clone())
                .unwrap_or_else(|| describe_conflict(&conflict.sides()).to_owned());
            let (ours, theirs) = ((this.clone(), conflict.clone()), (this.clone(), conflict));
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
                            .on_click(move |_, _, cx| {
                                let (this, conflict) = &ours;
                                this.update(cx, |this, cx| this.keep(conflict, Side::Ours, cx))
                            }),
                    )
                    .child(
                        Button::new("keep-all-theirs")
                            .ghost()
                            .small()
                            .font_weight(FontWeight::SEMIBOLD)
                            .label("Keep all of theirs")
                            .disabled(busy)
                            .on_click(move |_, _, cx| {
                                let (this, conflict) = &theirs;
                                this.update(cx, |this, cx| this.keep(conflict, Side::Theirs, cx))
                            }),
                    )
                    .into_any_element(),
            );
        }
        if let Some(message) = self.error() {
            banners.push(
                Notice::new(message.to_owned())
                    .alert()
                    .icon(IconName::TriangleAlert)
                    .into_any_element(),
            );
        }
        banners
    }

    /// The conflicts, or the sides to keep one of.
    fn render_body(&self, cx: &mut Context<Self>) -> AnyElement {
        if let Some(error) = self.query.error() {
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
        let Some(conflict) = self.conflict() else {
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
                let busy = self.is_busy();
                let kept = Arc::clone(&conflict);
                let resolution = cx.entity().downgrade();
                ConflictSides::new(conflict, busy, move |side, _, cx| {
                    let _ = resolution.update(cx, |this, cx| this.keep(&kept, side, cx));
                })
                .into_any_element()
            }
            // Loading until the viewer has the file on show, highlighted.
            ConflictKind::Text { .. } => match &self.viewer {
                Some(viewer) if viewer.read(cx).shown_text().is_some() => {
                    viewer.clone().into_any_element()
                }
                _ => loading(),
            },
        }
    }
}

impl Render for ConflictResolutionView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .min_h_0()
            .min_w_0()
            .child(self.render_body(cx))
    }
}

impl FileDiffView {
    /// Resolves files whole with `resolve` from now on: a test's.
    pub fn set_resolve_mutation(&mut self, resolve: ResolveFileMutation, cx: &mut Context<Self>) {
        self.resolution.update(cx, |resolution, cx| {
            resolution.set_resolve_mutation(resolve, cx)
        });
    }

    /// Whether the file on show is conflicted, its conflicts shown to resolve: an unstaged one.
    pub(super) fn is_resolving(&self) -> bool {
        *self.view.source() == DiffSource::Unstaged && self.view.conflicted()
    }

    /// The file on show, as its resolution is of it: `shown` when its conflicts are what's
    /// under the header.
    pub(super) fn resolution_file(&self, shown: bool) -> ResolutionFile {
        ResolutionFile {
            repository_id: self.view.repository_id().clone(),
            file: self.view.file(),
            file_key: self.view.file_key(),
            active: self.is_resolving(),
            shown,
            editing: self.editing.is_editing(),
            next: self.next_conflicted(),
        }
    }

    /// Tells the resolution which file it's of, as the view's synced.
    pub(super) fn sync_resolution(&mut self, shown: bool, cx: &mut Context<Self>) {
        let file = self.resolution_file(shown);
        self.resolution
            .update(cx, |resolution, cx| resolution.set_file(file, cx));
    }

    /// What the resolution tells the view: to open another file, or of editing in its viewer.
    pub(super) fn on_resolution(
        &mut self,
        event: &ConflictResolutionEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            ConflictResolutionEvent::Open { source, file } => cx.emit(FileDiffEvent::Open {
                source: source.clone(),
                file: file.clone(),
            }),
            ConflictResolutionEvent::Editing(session) => self.on_editing(session.clone(), cx),
            ConflictResolutionEvent::Edit => self.on_edit(cx),
            ConflictResolutionEvent::EditFailed(message) => {
                self.on_edit_failed(message.clone(), cx)
            }
            ConflictResolutionEvent::Shown => self.on_shown_for_editing(cx),
        }
        cx.notify();
    }

    /// The next conflicted file in the list after this one, or the first before it.
    fn next_conflicted(&self) -> Option<ChangedFile> {
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
mod tests {
    use gitto_git::features::conflicts::schema::ConflictSides;
    use gpui_kit::{AppContext as _, TestAppContext};

    use super::super::conflict_viewer::ConflictProgress;
    use super::super::test_views::{
        Opened, conflicted, id, init, open, reload_conflict_as, save, set_changes, set_conflict,
        side, text_conflict,
    };
    use super::*;
    use crate::components::ui::{ConfirmDialog, Tone};
    use crate::data::QueryClient;
    use crate::data::file_diff::{FilePatch, file_patch_key};

    const MARKERS: &str = "<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> side\n";

    /// `count` conflicts, each after a numbered line.
    fn conflicts(count: usize) -> String {
        (1..=count)
            .map(|line| format!("line {line}\n{MARKERS}"))
            .collect()
    }

    fn unstaged(file: &str) -> FileDiffEvent {
        FileDiffEvent::Open {
            source: DiffSource::Unstaged,
            file: conflicted(file),
        }
    }

    /// `a.txt` with `contents` (and `b.txt` conflicted too), open.
    fn open_conflict(contents: &str, cx: &mut TestAppContext) -> Opened {
        set_changes(vec![], vec![conflicted("a.txt"), conflicted("b.txt")], cx);
        set_conflict("a.txt", text_conflict(contents, "v1"), cx);
        set_conflict("b.txt", text_conflict(MARKERS, "v1"), cx);
        open(DiffSource::Unstaged, conflicted("a.txt"), cx)
    }

    /// What the header says of the conflicts left, if it does.
    fn left(opened: &Opened, cx: &mut TestAppContext) -> Option<String> {
        opened.read(cx, |view, cx| {
            view.resolution
                .read(cx)
                .shown_progress()
                .filter(|shown| shown.problem.is_none())
                .map(|shown| conflicts_left_label(shown.left))
        })
    }

    fn progress(opened: &Opened, cx: &mut TestAppContext) -> Option<ConflictProgress> {
        opened.read(cx, |view, cx| {
            view.resolution.read(cx).shown_progress().cloned()
        })
    }

    fn shown_text(opened: &Opened, cx: &mut TestAppContext) -> Option<String> {
        opened.read(cx, |view, cx| {
            let viewer = view.resolution.read(cx).viewer()?;
            viewer.read(cx).shown_text().map(str::to_owned)
        })
    }

    fn error(opened: &Opened, cx: &mut TestAppContext) -> Option<String> {
        opened.read(cx, |view, cx| {
            view.resolution.read(cx).error().map(str::to_owned)
        })
    }

    fn mark(version: &str, with_markers: bool) -> FileToResolve {
        FileToResolve {
            repository_id: id(),
            path: "a.txt".into(),
            resolution: ConflictResolution::Mark {
                version: Some(version.into()),
                with_markers,
            },
        }
    }

    // A file with text conflicts.

    #[gpui_kit::test]
    fn shows_its_conflicts_rather_than_a_patch_and_how_many_are_left(cx: &mut TestAppContext) {
        let _dir = init(cx);
        let opened = open_conflict(&conflicts(2), cx);

        assert_eq!(shown_text(&opened, cx), Some(conflicts(2)));
        assert_eq!(left(&opened, cx).as_deref(), Some("2 conflicts left"));
        // "Changed on both sides", with a side to keep whole.
        assert!(opened.has("keep-all-ours", cx));
        let patch = file_patch_key(&id(), &DiffSource::Unstaged, "a.txt");
        let loaded = cx.update(|cx| {
            QueryClient::get_entry::<FilePatch>(&patch, cx).is_some_and(|entry| {
                let entry = entry.read(cx);
                entry.is_fetching() || entry.data().is_some()
            })
        });
        assert!(!loaded);
        // Ours above theirs, one way only.
        assert!(!opened.has("split", cx));
        assert!(!opened.has("stage-file", cx));
    }

    #[gpui_kit::test]
    fn gives_the_viewer_the_text_it_read_without_copying_it_each_time(cx: &mut TestAppContext) {
        let _dir = init(cx);
        let opened = open_conflict(&conflicts(2), cx);
        let first = opened
            .read(cx, |view, cx| view.resolution.read(cx).disk())
            .expect("its text");
        opened.press("]", cx);
        let again = opened
            .read(cx, |view, cx| view.resolution.read(cx).disk())
            .expect("its text");
        assert!(Arc::ptr_eq(&first.contents, &again.contents));
        let viewer = opened.read(cx, |view, cx| {
            view.resolution.read(cx).viewer().cloned().unwrap()
        });
        assert!(cx.update(|cx| {
            let given = viewer.read(cx).given.clone();
            given.is_some_and(|(_, disk)| {
                disk.is_some_and(|disk| Arc::ptr_eq(&disk.contents, &first.contents))
            })
        }));
    }

    #[gpui_kit::test]
    fn follows_the_file_on_show_as_its_view_is_notified_and_drops_its_viewer_with_it(
        cx: &mut TestAppContext,
    ) {
        let _dir = init(cx);
        let opened = open_conflict(&conflicts(2), cx);
        let resolution = opened.read(cx, |view, _| view.resolution.clone());
        let viewer = cx.update(|cx| resolution.read(cx).viewer().map(Entity::downgrade));
        let viewer = viewer.expect("the conflicts' viewer");
        assert!(cx.update(|cx| resolution.read(cx).file.next == Some(conflicted("b.txt"))));

        // Not drawn again, the view tells its resolution of the next file as it's notified.
        opened.view.update(cx, |view, cx| {
            view.set_file(id(), DiffSource::Unstaged, conflicted("b.txt"), cx)
        });
        cx.run_until_parked();
        cx.update(|cx| {
            let resolution = resolution.read(cx);
            assert_eq!(resolution.file.file.path, "b.txt");
            assert_eq!(resolution.file.next, Some(conflicted("a.txt")));
        });

        // Staged, the file isn't resolved here: its viewer goes, and what it was told with it.
        opened.view.update(cx, |view, cx| {
            view.set_file(id(), DiffSource::Staged, conflicted("b.txt"), cx)
        });
        cx.run_until_parked();
        cx.update(|cx| {
            let resolution = resolution.read(cx);
            assert!(!resolution.file.active);
            assert!(resolution.viewer().is_none());
            assert!(resolution.viewer_events.is_none());
        });
        assert!(viewer.upgrade().is_none());
    }

    #[gpui_kit::test]
    fn is_only_marked_resolved_with_conflicts_left_once_the_user_says_they_belong_in_it(
        cx: &mut TestAppContext,
    ) {
        let _dir = init(cx);
        let opened = open_conflict(&conflicts(2), cx);

        opened.press("ctrl-enter", cx);
        assert!(opened.has_dialog(cx));
        opened.click("cancel", cx);
        assert!(!opened.has_dialog(cx));
        assert_eq!(opened.resolve.calls(), []);

        opened.click("mark-resolved", cx);
        assert!(opened.has_dialog(cx));
        opened.click(("action", 0usize), cx);
        assert_eq!(opened.resolve.calls(), [mark("v1", true)]);
        assert_eq!(opened.events(), [unstaged("b.txt")]);
    }

    #[gpui_kit::test]
    fn doesnt_take_another_files_progress_for_its_own(cx: &mut TestAppContext) {
        let _dir = init(cx);
        let opened = open_conflict("resolved\n", cx);
        assert_eq!(left(&opened, cx).as_deref(), Some("No conflicts left"));

        // The viewer still says how the last file went, while this one's conflicts are highlighted.
        let resolution = opened.read(cx, |view, _| view.resolution.clone());
        resolution.update(cx, |resolution, cx| {
            resolution.on_viewer(
                &ConflictViewerEvent::Progress(ConflictProgress {
                    file_key: Some(gitto_ui::git::diff_source::diff_file_key(
                        &DiffSource::Unstaged,
                        "b.txt",
                    )),
                    left: 0,
                    current: None,
                    saving: false,
                    version: Some("v9".into()),
                    problem: None,
                }),
                cx,
            )
        });
        assert!(!opened.is_enabled("mark-resolved", cx));
        assert_ne!(left(&opened, cx).as_deref(), Some("No conflicts left"));
    }

    #[gpui_kit::test]
    fn leaves_its_keys_to_the_editor_while_its_edited_by_hand(cx: &mut TestAppContext) {
        let _dir = init(cx);
        let opened = open_conflict(&conflicts(2), cx);
        opened.click("edit-file", cx);
        let edited = |cx: &mut TestAppContext| {
            opened.read(cx, |view, cx| {
                let viewer = view.resolution.read(cx).viewer()?;
                let editor = viewer.read(cx).whole_file()?;
                Some(editor.read(cx).value().to_string())
            })
        };
        assert_eq!(edited(cx), Some(conflicts(2)));

        // Typed, rather than keeping ours or opening the next conflicted file.
        opened.press("o", cx);
        opened.press("n", cx);
        assert!(
            edited(cx)
                .is_some_and(|text| text.contains("on") && text.len() == conflicts(2).len() + 2)
        );
        assert_eq!(opened.events(), []);
        assert_eq!(opened.resolve.calls(), []);
    }

    #[gpui_kit::test]
    fn leaves_its_keys_to_a_dialog_or_menu_on_top_of_it(cx: &mut TestAppContext) {
        let _dir = init(cx);
        let opened = open_conflict(&conflicts(2), cx);
        cx.update_window(opened.window, |_, window, cx| {
            ConfirmDialog::new(IconName::TriangleAlert, Tone::Amber, "A dialog").open(window, cx)
        })
        .unwrap();
        cx.run_until_parked();

        opened.press("t", cx);
        opened.press("]", cx);
        assert_eq!(opened.tree.saves(), []);
        assert_eq!(
            progress(&opened, cx).and_then(|shown| shown.current),
            Some(1)
        );
    }

    #[gpui_kit::test]
    fn is_marked_resolved_once_none_are_then_the_next_conflicted_file_opens(
        cx: &mut TestAppContext,
    ) {
        let _dir = init(cx);
        let opened = open_conflict("resolved\n", cx);
        assert_eq!(left(&opened, cx).as_deref(), Some("No conflicts left"));

        opened.click("mark-resolved", cx);
        assert_eq!(opened.resolve.calls(), [mark("v1", false)]);
        assert_eq!(opened.events(), [unstaged("b.txt")]);
    }

    #[gpui_kit::test]
    fn opens_its_staged_changes_once_it_was_the_last_conflicted_file(cx: &mut TestAppContext) {
        let _dir = init(cx);
        set_changes(vec![], vec![conflicted("a.txt")], cx);
        set_conflict("a.txt", text_conflict("resolved\n", "v1"), cx);
        let opened = open(DiffSource::Unstaged, conflicted("a.txt"), cx);
        assert_eq!(left(&opened, cx).as_deref(), Some("No conflicts left"));

        opened.press("ctrl-enter", cx);
        assert_eq!(
            opened.events(),
            [FileDiffEvent::Open {
                source: DiffSource::Staged,
                file: ChangedFile {
                    status: FileStatus::Modified,
                    ..conflicted("a.txt")
                },
            }]
        );
    }

    #[gpui_kit::test]
    fn says_why_it_couldnt_be_marked_resolved(cx: &mut TestAppContext) {
        let _dir = init(cx);
        let opened = open_conflict("resolved\n", cx);
        opened.resolve.fail_once(
            "a.txt still has conflict markers. Resolve its conflicts, then mark it resolved.",
        );

        opened.click("mark-resolved", cx);
        assert_eq!(
            error(&opened, cx).as_deref(),
            Some("a.txt still has conflict markers. Resolve its conflicts, then mark it resolved.")
        );
        assert_eq!(opened.events(), []);
    }

    #[gpui_kit::test]
    fn steps_through_its_conflicts_and_resolves_them_with_the_keyboard(cx: &mut TestAppContext) {
        let _dir = init(cx);
        let opened = open_conflict(&conflicts(3), cx);
        let current = |cx: &mut TestAppContext| progress(&opened, cx).and_then(|p| p.current);
        assert_eq!(current(cx), Some(1));

        opened.press("]", cx);
        assert_eq!(current(cx), Some(2));
        opened.press("[", cx);
        opened.press("[", cx);
        // Round to the other end.
        assert_eq!(current(cx), Some(3));

        // Ours of the third, then on to the second: the one before, as it was the last.
        opened.press("o", cx);
        let third = conflicts(3).replace(&format!("line 3\n{MARKERS}"), "line 3\nours\n");
        assert_eq!(opened.tree.saves(), [save("a.txt", &third, "v1", false)]);
        assert_eq!(current(cx), Some(2));
        opened.press("t", cx);
        let second = third.replace(&format!("line 2\n{MARKERS}"), "line 2\ntheirs\n");
        assert_eq!(
            opened.tree.saves().last(),
            Some(&save("a.txt", &second, "v2", false))
        );
        opened.press("b", cx);
        let both = second.replace(&format!("line 1\n{MARKERS}"), "line 1\nours\ntheirs\n");
        assert_eq!(
            opened.tree.saves().last(),
            Some(&save("a.txt", &both, "v2", false))
        );
        assert_eq!(left(&opened, cx).as_deref(), Some("No conflicts left"));

        opened.press("n", cx);
        assert_eq!(opened.events(), [unstaged("b.txt")]);
    }

    #[gpui_kit::test]
    fn writes_a_resolved_conflict_over_the_version_it_was_read_at(cx: &mut TestAppContext) {
        let _dir = init(cx);
        let opened = open_conflict(MARKERS, cx);

        opened.click(("keep-ours", 0usize), cx);
        assert_eq!(opened.tree.saves(), [save("a.txt", "ours\n", "v1", false)]);
        assert_eq!(
            progress(&opened, cx).and_then(|p| p.version).as_deref(),
            Some("v2")
        );
    }

    #[gpui_kit::test]
    fn says_why_a_conflict_couldnt_be_resolved(cx: &mut TestAppContext) {
        let _dir = init(cx);
        let opened = open_conflict(MARKERS, cx);
        opened.tree.reply(|_| {
            Err(QueryError::msg(
                "a.txt was changed on disk since you started editing it.",
            ))
        });

        opened.click(("keep-theirs", 0usize), cx);
        assert_eq!(
            error(&opened, cx).as_deref(),
            Some("a.txt was changed on disk since you started editing it.")
        );
        // Read again, to be shown as it is now.
        assert_eq!(opened.tree.reloads(), [keys::conflict(&id(), "a.txt")]);
        // Still as it was.
        assert_eq!(left(&opened, cx).as_deref(), Some("1 conflict left"));
    }

    #[gpui_kit::test]
    fn keeps_a_side_whole_of_the_version_shown(cx: &mut TestAppContext) {
        let _dir = init(cx);
        let opened = open_conflict(&conflicts(2), cx);

        opened.click("keep-all-theirs", cx);
        assert_eq!(
            opened.resolve.calls(),
            [FileToResolve {
                repository_id: id(),
                path: "a.txt".into(),
                resolution: ConflictResolution::Keep {
                    side: Side::Theirs,
                    sides: ConflictSides {
                        base: Some(side("b1")),
                        ours: Some(side("o1")),
                        theirs: Some(side("t1")),
                    },
                    version: Some("v1".into()),
                },
            }]
        );
        assert_eq!(opened.events(), [unstaged("b.txt")]);
    }

    #[gpui_kit::test]
    fn isnt_resolved_whole_while_its_edited_as_the_edits_arent_known_yet(cx: &mut TestAppContext) {
        let _dir = init(cx);
        let opened = open_conflict("resolved\n", cx);
        assert_eq!(left(&opened, cx).as_deref(), Some("No conflicts left"));

        opened.click("edit-file", cx);
        assert!(opened.read(cx, |view, _| view.editing.is_editing()));
        assert!(!opened.is_enabled("mark-resolved", cx));
        assert!(!opened.is_enabled("keep-all-ours", cx));
        opened.press("ctrl-enter", cx);
        assert_eq!(opened.resolve.calls(), []);
    }

    #[gpui_kit::test]
    fn is_edited_by_hand_and_read_again_once_that_stops_before_its_shown(cx: &mut TestAppContext) {
        let _dir = init(cx);
        let opened = open_conflict(&conflicts(2), cx);

        opened.click("edit-file", cx);
        assert!(opened.read(cx, |view, _| view.editing.is_editing()));
        // What's read while it's edited isn't for the viewer.
        assert_eq!(
            opened.read(cx, |view, cx| view.resolution.read(cx).disk()),
            None
        );
        assert!(opened.read(cx, |view, cx| {
            view.resolution
                .read(cx)
                .viewer()
                .is_some_and(|viewer| viewer.read(cx).whole_file().is_some())
        }));

        reload_conflict_as("a.txt", text_conflict("resolved\n", "v3"), cx);
        opened.click("edit-file", cx);
        assert_eq!(
            opened.read(cx, |view, cx| view.resolution.read(cx).disk()),
            Some(DiskFile {
                contents: "resolved\n".into(),
                version: "v3".into(),
            })
        );
        assert_eq!(shown_text(&opened, cx).as_deref(), Some("resolved\n"));
        assert_eq!(
            progress(&opened, cx).and_then(|p| p.version).as_deref(),
            Some("v3")
        );
    }

    // A file that isn't text on both sides.

    #[gpui_kit::test]
    fn says_what_each_side_did_and_keeps_one_whole(cx: &mut TestAppContext) {
        let _dir = init(cx);
        set_changes(vec![], vec![conflicted("gone.txt")], cx);
        let conflict = Conflict {
            theirs: None,
            ..text_conflict("ours\n", "v1")
        };
        assert_eq!(
            describe_conflict(&conflict.sides()),
            "Deleted in theirs, changed in ours"
        );
        set_conflict("gone.txt", conflict, cx);
        let opened = open(DiffSource::Unstaged, conflicted("gone.txt"), cx);

        opened.click("keep-theirs", cx);
        let calls = opened.resolve.calls();
        assert!(matches!(
            &calls[..],
            [FileToResolve {
                path,
                resolution: ConflictResolution::Keep {
                    side: Side::Theirs,
                    version: Some(version),
                    ..
                },
                ..
            }] if path == "gone.txt" && version == "v1"
        ));
        // It was the last one: its staged changes, its deletion, are next.
        assert!(matches!(
            &opened.events()[..],
            [FileDiffEvent::Open { source: DiffSource::Staged, file }] if file.path == "gone.txt"
        ));
        assert!(!opened.has("mark-resolved", cx));
    }

    #[gpui_kit::test]
    fn says_a_binary_file_cant_be_shown_as_text(cx: &mut TestAppContext) {
        let _dir = init(cx);
        set_changes(vec![], vec![conflicted("gone.txt")], cx);
        set_conflict(
            "gone.txt",
            Conflict {
                text: None,
                binary: true,
                unreadable: Some("This file is binary.".into()),
                ..text_conflict(MARKERS, "v1")
            },
            cx,
        );
        let opened = open(DiffSource::Unstaged, conflicted("gone.txt"), cx);

        assert!(opened.is_enabled("keep-ours", cx));
        assert!(opened.is_enabled("keep-theirs", cx));
        assert!(!opened.has("mark-resolved", cx));
    }
}
