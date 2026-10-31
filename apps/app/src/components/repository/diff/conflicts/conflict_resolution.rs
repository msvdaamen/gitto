//! Resolving the conflicted file on show, in its view. Ported from `conflict-resolution.tsx`.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use gitto_git::ContentVersion;
use gitto_git::FileStatus;
use gitto_git::features::conflicts::schema::{Conflict, FileVersion, Side};
use gitto_git::features::diff::schema::ChangedFile;
use gitto_git::input::RepositoryId;
use gitto_ui_logic::git::conflicts::{ConflictKind, conflict_kind, describe_conflict};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Disableable as _, Sizable as _, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Context, Entity, EventEmitter, FontWeight, Subscription, Task, Window, div,
};

use super::super::file_diff_view::{FileDiffEvent, FileDiffView};
use super::super::file_editing::{EditSession, FileIo, Overwrite, Reload, SaveRequest};
use super::conflict_sides::ConflictSides;
use super::conflict_viewer::{
    ConflictProgress, ConflictViewer, ConflictViewerEvent, DiskFile, Resolution, SaveResolved,
};
use super::markers_dialog::open_markers_dialog;
use gitto_app_data::data::conflicts::{ConflictResolution, FileToResolve, resolve_file_mutation};
use gitto_app_data::data::keyed::Hold;
use gitto_app_data::data::{DataError, Loaded, Mutation, Part, Shown};
use gitto_app_kit::theme::PaletteExt as _;
use gitto_app_kit::ui::{EmptyState, IconButton, IconName, LoadingState, Notice, action_button};
use gitto_ui_logic::git::diff_source::{DiffFileKey, DiffSource};

/// Resolves a conflicted file whole: keeps a side, or marks it resolved.
pub type ResolveFileMutation = Entity<Mutation<FileToResolve>>;

/// The file a resolution is of, as the file's view has it (see
/// [`ConflictResolutionView::set_file`]).
#[derive(Debug, Clone, PartialEq)]
pub struct ResolutionFile {
    pub repository_id: RepositoryId,
    /// The file on show, and its key.
    pub file: ChangedFile,
    pub file_key: DiffFileKey,
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
    /// The repository the file's in, rendering it again as its conflicted files are read.
    repository: Shown,
    /// Holds the conflicted file while it's resolved, as it's loaded (see `held`).
    hold: Option<Hold>,
    /// What's held: the repository and the file's path, and whether it's held at all.
    held: (RepositoryId, String, bool),
    /// The conflicted file last read, by its path, and which of its reads it is, as last synced:
    /// while another one's loads, the last one's stays on show, until it's in. Taken once per
    /// read, as it's the whole file.
    conflict: Option<(String, u64, Arc<Conflict>)>,
    /// The text of the conflict read last, if it's resolved in its text: worked out once per
    /// read, rather than each time it's read, as it's the whole file.
    text: Option<(Arc<Conflict>, DiskFile)>,
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
    error: Option<(DiffFileKey, String)>,
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

/// What it shows of the repository: the conflicted files read.
const PARTS: &[Part] = &[Part::Conflicts];

impl ConflictResolutionView {
    pub fn new(
        file: ResolutionFile,
        io: Rc<RefCell<FileIo>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let repository = Shown::new(&file.repository_id, PARTS, cx);
        let hold = file.active.then(|| {
            repository.update(cx, |repository, cx| {
                repository.hold_conflict(&file.file.path, cx)
            })
        });
        let resolve = resolve_file_mutation(cx);
        let this = cx.entity();
        let subscriptions = vec![
            cx.observe(&resolve, |_, _, cx| cx.notify()),
            // Notified, what it shows may have changed: it's synced before it renders again.
            cx.observe_in(&this, window, |this, _, window, cx| this.sync(window, cx)),
        ];
        let mut resolution = ConflictResolutionView {
            held: (
                file.repository_id.clone(),
                file.file.path.clone(),
                file.active,
            ),
            file,
            repository,
            hold,
            conflict: None,
            text: None,
            reads: 0,
            load_error: None,
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
        self.conflict
            .as_ref()
            .map(|(_, _, conflict)| Arc::clone(conflict))
    }

    /// Whether the conflict on show is the last file's, while this one's loads.
    fn is_placeholder(&self) -> bool {
        self.conflict
            .as_ref()
            .is_some_and(|(path, _, _)| *path != self.held.1)
    }

    /// Why the last thing done to the conflict failed, if it's of the file on show.
    pub fn error(&self) -> Option<&str> {
        self.error
            .as_ref()
            .filter(|(key, _)| *key == self.file.file_key)
            .map(|(_, message)| message.as_str())
    }

    fn is_text(&self) -> bool {
        self.conflict.as_ref().is_some_and(|(_, _, conflict)| {
            matches!(conflict_kind(conflict), ConflictKind::Text { .. })
        })
    }

    /// Brings what it says up to date with what the repository has read of the file at `path`:
    /// the conflict is taken once per read.
    fn sync_loaded(&mut self, path: &str, cx: &App) {
        let loaded = self.repository.read(cx).conflict(path);
        let reads = loaded.map_or(0, Loaded::updates);
        let taken = self
            .conflict
            .as_ref()
            .is_some_and(|(taken, read, _)| taken == path && *read == reads);
        if !taken && let Some(conflict) = loaded.and_then(Loaded::data) {
            self.conflict = Some((path.to_owned(), reads, Arc::new(conflict.clone())));
        }
        self.reads = reads;
        self.load_error = loaded
            .and_then(Loaded::error)
            .filter(|_| self.held.2)
            .cloned();
    }

    /// Takes the text of the conflict loaded last, unless it has already.
    fn sync_text(&mut self) {
        let loaded = self.conflict();
        let taken = self.text.as_ref().map(|(conflict, _)| conflict);
        if loaded.as_ref().map(Arc::as_ptr) == taken.map(Arc::as_ptr) {
            return;
        }
        self.text = loaded.and_then(|conflict| {
            let ConflictKind::Text { contents, version } = conflict_kind(&conflict) else {
                return None;
            };
            let disk = DiskFile {
                version: version.clone(),
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
        let held = (self.file.repository_id.clone(), path.clone(), active);
        if self.held != held {
            self.repository.set(&held.0, cx);
            self.hold = active.then(|| {
                self.repository
                    .update(cx, |repository, cx| repository.hold_conflict(&path, cx))
            });
            self.held = held;
        }
        self.sync_loaded(&path, cx);
        // After editing, the file is read again before it's shown.
        if active && self.was_editing && !editing {
            self.read_after = Some(self.reads);
            self.repository.update(cx, |repository, cx| {
                drop(repository.reload_conflict(&path, cx))
            });
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
                .unwrap_or_else(|_| Task::ready(Err(DataError::msg("The file was closed."))))
        });
        let viewer = cx.new(|cx| ConflictViewer::new(save, cx));
        self.viewer_events =
            Some(cx.subscribe(&viewer, |this, _, event, cx| this.on_viewer(event, cx)));
        self.viewer = Some(viewer.clone());
        viewer
    }

    pub(in crate::components::repository::diff) fn on_viewer(
        &mut self,
        event: &ConflictViewerEvent,
        cx: &mut Context<Self>,
    ) {
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
    pub(in crate::components::repository::diff) fn disk(&self) -> Option<DiskFile> {
        if self.file.editing || self.is_placeholder() {
            return None;
        }
        if let Some(after) = self.read_after
            && self.reads <= after
        {
            return None;
        }
        self.text.as_ref().map(|(_, disk)| disk.clone())
    }

    /// The viewer's progress, once it's of this file: the last one's is still on show while this
    /// one's conflicts are being highlighted.
    pub fn shown_progress(&self) -> Option<&ConflictProgress> {
        let progress = self.progress.as_ref()?;
        (self.is_text() && progress.file_key.as_ref() == Some(&self.file.file_key))
            .then_some(progress)
    }

    /// Whether the file can't be resolved whole now: while it's being, nor while the last file's
    /// still on show, as this one's loads, nor before this one's version on disk is known, which a
    /// write is checked against (just after editing, say). Nor while it's edited: it's resolved
    /// as the edits leave it, which is only known once editing stops.
    fn is_busy(&self) -> bool {
        self.resolving
            || self.file.editing
            || self.is_placeholder()
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

    /// The version of the file's text to check before marking it resolved: the latest one written
    /// here, or else the one loaded.
    fn text_version(&self) -> Option<ContentVersion> {
        self.shown_progress()
            .and_then(|shown| shown.version.clone())
            .or_else(|| {
                self.conflict()
                    .and_then(|conflict| Some(conflict.file.text()?.version.clone()))
            })
    }

    /// The version of the file to check before keeping a side: the latest one written here, or
    /// else the loaded file's (see `ConflictFile::version`), which isn't always of its text.
    fn file_version(&self) -> Option<FileVersion> {
        self.shown_progress()
            .and_then(|shown| shown.version.clone())
            .map(FileVersion::Content)
            .or_else(|| self.conflict().and_then(|conflict| conflict.file.version()))
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
                    Ok(Err(error)) => this.error = Some((key, error.to_string())),
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
        let version = self.text_version();
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
                    let version = this.text_version();
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
        let version = self.file_version();
        self.resolve_whole(
            ConflictResolution::Keep {
                side,
                sides: conflict.sides.clone(),
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
        expected: ContentVersion,
        cx: &mut Context<Self>,
    ) -> Task<Result<ContentVersion, DataError>> {
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
                cx.update(|cx| (io.reload)(&id, Reload::Conflict(path), cx));
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
            && !self.is_placeholder()
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
                            .icon(gitto_app_kit::ui::Icon::new(IconName::Check))
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
                .unwrap_or_else(|| describe_conflict(&conflict.sides).to_owned());
            let (ours, theirs) = ((this.clone(), conflict.clone()), (this.clone(), conflict));
            banners.push(
                Notice::new("conflict", message)
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
                Notice::new("conflict-error", message.to_owned())
                    .alert()
                    .icon(IconName::TriangleAlert)
                    .into_any_element(),
            );
        }
        banners
    }

    /// The conflicts, or the sides to keep one of.
    fn render_body(&self, cx: &mut Context<Self>) -> AnyElement {
        if let Some(error) = &self.load_error {
            return EmptyState::new(
                "conflict-error",
                IconName::TriangleAlert,
                "Couldn't load the conflict",
            )
            .message(error.to_string())
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
                ConflictSides::new(conflict, &self.file.file.path, busy, move |side, _, cx| {
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
    pub(in crate::components::repository::diff) fn is_resolving(&self) -> bool {
        *self.view.source() == DiffSource::Unstaged && self.view.conflicted()
    }

    /// The file on show, as its resolution is of it: `shown` when its conflicts are what's
    /// under the header.
    pub(in crate::components::repository::diff) fn resolution_file(
        &self,
        shown: bool,
    ) -> ResolutionFile {
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
    pub(in crate::components::repository::diff) fn sync_resolution(
        &mut self,
        shown: bool,
        cx: &mut Context<Self>,
    ) {
        let file = self.resolution_file(shown);
        self.resolution
            .update(cx, |resolution, cx| resolution.set_file(file, cx));
    }

    /// What the resolution tells the view: to open another file, or of editing in its viewer.
    pub(in crate::components::repository::diff) fn on_resolution(
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
mod tests;
