//! A file's changes, in place of the history. Ported from `file-diff-view.tsx`.

use std::rc::Rc;
use std::time::Duration;

use gitto_git::FileStatus;
use gitto_git::features::diff::schema::ChangedFile;
use gitto_git::input::RepositoryId;
use gitto_ui_logic::git::changes::{counts_of, has_patch, total_lines, unchanged_reason};
use gpui_kit::component::button::Button;
use gpui_kit::component::{Disableable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    Animation, AnimationExt as _, AnyElement, AnyWindowHandle, App, Context, ElementId, Entity,
    EventEmitter, FocusHandle, Focusable, FontWeight, KeyDownEvent, SharedString, Subscription,
    Task, Transformation, Window, div, percentage, px, rems,
};

use super::conflicts::conflict_resolution::FileResolution;
use super::conflicts::conflict_viewer::prepare_conflicts;
use super::file_editing::FileEditing;
use super::file_staging::{FileStaging, StageFileMutation, StageLinesMutation};
use super::shown_file::{ShownFile, patch_cache_key};
use super::viewer::diff_metadata::Side;
use super::viewer::patch_viewer::{PatchProps, PatchViewer, PatchViewerEvent};
use super::viewer::viewer_editing::{PatchEditor, PatchEditorEvent};
use super::viewer::viewer_runtime::{LoadFile, LoadedFile, prepare_patch};
use gitto_app_data::data::Repositories;
use gitto_app_data::data::diff::fetch_blob;
use gitto_app_data::data::file_diff::{FilePatch, fetch_file_patch};
use gitto_app_data::data::staging::StageAction;
use gitto_app_data::state::diff_style::{DiffStyle, DiffStyleSetting};
use gitto_app_kit::theme::PaletteExt as _;
use gitto_app_kit::ui::{
    EmptyState, FileStatusBadge, Icon, IconButton, IconName, LineStats, LoadingState, Notice,
    action_button,
};
use gitto_ui_logic::git::diff_source::{DiffSource, diff_file_key};

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

/// Changed lines above which a file's changes aren't loaded ahead (see [`prefetch_file_diff`]):
/// parsing them starts to take a while, for a file that may not be opened after all.
pub const PREFETCH_DIFF_LINES: u64 = 5_000;

/// Loads and highlights a file's changes ahead of opening them, e.g. when it's pointed at, so they
/// show at once (`prefetchFileDiff`). Not for one that's binary, or large, nor for one without line
/// counts (untracked, say), which can be of any size: an untracked log that's being written to
/// would be read whole again each time it's pointed at. An uncommitted file's are named by what's
/// in them in the highlighting cache, and reloaded like the status once the working tree
/// changes, so what's loaded ahead is never shown stale. A conflicted file's conflicts are loaded
/// instead.
pub fn prefetch_file_diff(
    repository_id: &RepositoryId,
    source: &DiffSource,
    file: &ChangedFile,
    uncounted: bool,
    cx: &mut App,
) {
    if file.status == FileStatus::Conflicted && *source == DiffSource::Unstaged {
        // Its conflicts, highlighted as the viewer names them, so they show at once.
        let fetching = Repositories::open(repository_id, cx).update(cx, |repository, cx| {
            repository.fetch_conflict(&file.path, cx)
        });
        let file_key = diff_file_key(source, &file.path);
        let path = file.path.clone();
        cx.spawn(async move |cx| {
            // Only ahead of time: opening it says what went wrong.
            if let Ok(conflict) = fetching.await
                && let Some(text) = conflict.text
            {
                cx.update(|cx| {
                    prepare_conflicts(&file_key, &path, text.contents, &text.version, cx)
                });
            }
        })
        .detach();
        return;
    }
    let Some(counts) = counts_of(file) else {
        return;
    };
    if !has_patch(file, uncounted) || total_lines(counts) > PREFETCH_DIFF_LINES {
        return;
    }
    let fetching = fetch_file_patch(repository_id, source, file, cx);
    let source = source.clone();
    cx.spawn(async move |cx| {
        // Only ahead of time: opening it says what went wrong.
        let Ok(data) = fetching.await else {
            return;
        };
        if data.patch.is_empty() {
            return;
        }
        let key = patch_cache_key(&source, &data);
        let _ = cx
            .update(|cx| prepare_patch(data.patch.clone(), key, cx))
            .await;
    })
    .detach();
}

/// What the view shows under its header.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Body {
    /// The file has no changes left on its side.
    NoLongerChanged,
    Conflict,
    Nested,
    Binary,
    NoChangedLines,
    Large(u64),
    /// Why its changes couldn't be shown.
    Error(String),
    Loading,
    Patch,
}

/// The changes in one file of a commit, a stash or the uncommitted changes, in place of the
/// history. Escape, or the back button, goes back to the history; the other files in the same
/// list are a step away. Uncommitted changes are kept up to date as they change on disk, in place;
/// once the file has none left on its side, that's said instead.
///
/// An uncommitted file can be staged, or unstaged, whole, or its lines a hunk or a selection at a
/// time (see `file_staging`), one at a time; an unstaged one can be edited (see `file_editing`).
/// Once that leaves it without changes on its side, the next file in its list opens. A conflicted
/// one shows its conflicts instead, to resolve (see `conflict_resolution`).
///
/// An entity the page keeps while a file's changes are open, telling it which
/// ([`set_file`](Self::set_file)): going to another file, the last one stays on show, header and
/// all, until the new one's changes are in. The page asks it, before closing it or opening
/// another, whether it may ([`may_leave`](Self::may_leave)).
pub struct FileDiffView {
    pub(super) view: ShownFile,
    pub(super) staging: FileStaging,
    focus: FocusHandle,
    viewer: Option<Entity<PatchViewer>>,
    /// The patch last given to the viewer, which it says is on show.
    viewer_patch: Option<FilePatch>,
    /// Why a file's patch couldn't be shown, and its key: another file is tried afresh.
    viewer_error: Option<(String, String)>,
    /// Editing the file by hand, in its diff or its conflicts.
    pub(super) editing: FileEditing,
    /// The file being edited in the diff's place, while it is.
    pub(super) patch_editor: Option<Entity<PatchEditor>>,
    /// Resolving a conflicted file.
    pub(super) resolution: FileResolution,
    /// The file on show whose neighbours were loaded ahead.
    prefetched_for: Option<String>,
    /// What had the focus when the changes opened, to give it back once they close.
    opener: Option<FocusHandle>,
    pub(super) window: AnyWindowHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<FileDiffEvent> for FileDiffView {}

impl Focusable for FileDiffView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl FileDiffView {
    pub fn new(
        repository_id: RepositoryId,
        source: DiffSource,
        file: ChangedFile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let staging = FileStaging::new(cx);
        FileDiffView::with_staging(repository_id, source, file, staging, window, cx)
    }

    /// The view, staging with the mutations given: a test's.
    pub fn with_mutations(
        repository_id: RepositoryId,
        source: DiffSource,
        file: ChangedFile,
        stage_lines: StageLinesMutation,
        stage_file: StageFileMutation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let staging = FileStaging::with(stage_lines, stage_file, cx);
        FileDiffView::with_staging(repository_id, source, file, staging, window, cx)
    }

    fn with_staging(
        repository_id: RepositoryId,
        source: DiffSource,
        file: ChangedFile,
        staging: FileStaging,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let resolution = FileResolution::new(
            &repository_id,
            &file,
            file.status == FileStatus::Conflicted,
            cx,
        );
        let view = ShownFile::new(repository_id, source, file, cx);
        let editing = FileEditing::new(cx);
        let focus = cx.focus_handle();
        // The changes have the keyboard, so Esc goes back to the history; the focus goes back to
        // what opened them once they close with it inside: it would be lost to the page
        // otherwise.
        let opener = window.focused(cx);
        window.focus(&focus, cx);
        let subscriptions = vec![
            cx.observe_global::<DiffStyleSetting>(|_, cx| cx.notify()),
            cx.observe(view.busy(), |_, _, cx| cx.notify()),
            cx.on_release(|this: &mut Self, cx| this.give_focus_back(cx)),
        ];
        FileDiffView {
            view,
            staging,
            focus,
            viewer: None,
            viewer_patch: None,
            viewer_error: None,
            editing,
            patch_editor: None,
            resolution,
            prefetched_for: None,
            opener,
            window: window.window_handle(),
            _subscriptions: subscriptions,
        }
    }

    pub fn repository_id(&self) -> &RepositoryId {
        self.view.repository_id()
    }

    pub fn source(&self) -> &DiffSource {
        self.view.source()
    }

    pub fn file(&self) -> ChangedFile {
        self.view.file()
    }

    /// Shows `file` of `source` instead, e.g. the next one in its list: the last one stays on show
    /// until its changes are in.
    pub fn set_file(
        &mut self,
        repository_id: RepositoryId,
        source: DiffSource,
        file: ChangedFile,
        cx: &mut Context<Self>,
    ) {
        self.view.set_file(repository_id, source, file, cx);
        cx.notify();
    }

    /// Resolves to whether the file may be closed, or another opened, once its edits are saved,
    /// or the user says what to do with them (TS's `onGuard`, see `FileDiffView::leave`).
    pub fn may_leave(&mut self, cx: &mut Context<Self>) -> Task<bool> {
        let leaving = self.leave(cx);
        cx.spawn(async move |_, _| leaving.await)
    }

    /// The viewer showing the patch, while one is.
    pub fn viewer(&self) -> Option<&Entity<PatchViewer>> {
        self.viewer.as_ref()
    }

    /// Gives the focus back to what opened the changes, if it's still on the page and the focus
    /// was in them, as they close.
    fn give_focus_back(&mut self, cx: &mut App) {
        let Some(opener) = self.opener.take() else {
            return;
        };
        let focus = self.focus.clone();
        let viewer = self
            .viewer
            .as_ref()
            .map(|viewer| viewer.read(cx).focus_handle(cx));
        let window = self.window;
        cx.defer(move |cx| {
            let _ = window.update(cx, |_, window, cx| {
                let focused = window.focused(cx);
                let lost = focused
                    .as_ref()
                    .is_none_or(|focused| *focused == focus || viewer.as_ref() == Some(focused));
                if lost {
                    window.focus(&opener, cx);
                }
            });
        });
    }

    /// What's under the header.
    fn body(&self) -> Body {
        let view = &self.view;
        if view.gone() || view.empty() {
            return Body::NoLongerChanged;
        }
        if view.conflicted() {
            return Body::Conflict;
        }
        if view.nested() {
            return Body::Nested;
        }
        if view.binary() {
            return Body::Binary;
        }
        let lines = view.lines();
        if lines == Some(0) {
            return Body::NoChangedLines;
        }
        if view.large() {
            return Body::Large(lines.unwrap_or(0));
        }
        let key = view.file_key();
        if let Some((failed, message)) = &self.viewer_error
            && *failed == key
        {
            return Body::Error(message.clone());
        }
        if let Some(error) = view.patch_error() {
            return Body::Error(error.message());
        }
        // Another file's patch is only kept while it's on show: one isn't started with it.
        match view.patch() {
            Some(data) if data.key == key || view.shown_patch().is_some() => Body::Patch,
            _ => Body::Loading,
        }
    }

    /// Brings the view up to date with what's loaded: the file's place in its list, the viewer
    /// (made while the patch is on show, given what to show), the conflicts' viewer, and the
    /// files a step away, loaded ahead.
    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.view.sync(cx);
        let body = self.body();
        if body == Body::Patch {
            self.show_patch(cx);
        } else if self.viewer.take().is_some() {
            self.viewer_patch = None;
            // No reload to finish: the file has no changes left to show, say.
            self.cancel_reload();
            self.view.patch_left(cx);
        }
        self.sync_patch_editor(body == Body::Patch, window, cx);
        self.sync_resolution(body == Body::Conflict, window, cx);
        // The files a step away are loaded ahead once another one's on show, so stepping to them
        // shows them at once. Not again when the same one's patch is reloaded, as an uncommitted
        // file's is whenever the working tree changes.
        let shown_key = self.view.shown_patch().map(|shown| shown.key.clone());
        if shown_key.is_some() && shown_key != self.prefetched_for {
            self.prefetched_for = shown_key;
            let uncounted = self.view.lists().uncounted();
            let id = self.view.repository_id().clone();
            let source = self.view.source().clone();
            for near in [self.view.previous(), self.view.next()]
                .into_iter()
                .flatten()
            {
                prefetch_file_diff(&id, &source, &near, uncounted, cx);
            }
        }
    }

    /// Gives the viewer the patch to show, making it the first time.
    fn show_patch(&mut self, cx: &mut Context<Self>) {
        let Some(data) = self.view.patch() else {
            return;
        };
        let viewer = match &self.viewer {
            Some(viewer) => viewer.clone(),
            None => {
                let viewer = cx.new(|cx| PatchViewer::new(self.load_file(&data), cx));
                self._subscriptions
                    .push(cx.subscribe(&viewer, |this, _, event, cx| this.on_viewer(event, cx)));
                self.viewer = Some(viewer.clone());
                viewer
            }
        };
        let props = PatchProps {
            patch: data.patch.clone(),
            file_key: data.key.clone(),
            cache_key: patch_cache_key(self.view.source(), &data),
            style: DiffStyle::current(cx),
            staging: self.line_staging(cx),
        };
        let load_file = self.load_file(&data);
        self.viewer_patch = Some(data);
        viewer.update(cx, |viewer, cx| {
            viewer.set_load_file(load_file);
            viewer.set_props(props, cx);
        });
    }

    /// Edits the file in the diff's place while it's edited: from the patch on show, read whole.
    fn sync_patch_editor(
        &mut self,
        shows_patch: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let editing = self.editing.is_editing() && shows_patch && !self.view.conflicted();
        if !editing {
            let Some(editor) = self.patch_editor.take() else {
                return;
            };
            // The keyboard stays with the view, for Escape to close it, say.
            let focused = editor
                .read(cx)
                .editor()
                .is_some_and(|editor| editor.focus_handle(cx).is_focused(window));
            if focused {
                window.focus(&self.focus, cx);
            }
            if self.editing.is_editing() && editor.read(cx).editor().is_some() {
                // Taken off the page while it's edited: that's over, and what's left is saved.
                self.on_editing(None, cx);
            }
            return;
        }
        if self.patch_editor.is_some() {
            return;
        }
        let shown = self
            .viewer
            .as_ref()
            .and_then(|viewer| viewer.read(cx).shown_diff().cloned());
        let (Some(diff), Some(data)) = (shown, self.viewer_patch.clone()) else {
            self.on_edit_failed("This file can't be edited.".to_owned(), cx);
            return;
        };
        let load_file = self.load_file(&data);
        let path = data.file.path.clone();
        let editor = cx.new(|cx| PatchEditor::new(diff, path, load_file, window, cx));
        self._subscriptions
            .push(cx.subscribe(&editor, |this, _, event, cx| match event {
                PatchEditorEvent::Editing(session) => this.on_editing(session.clone(), cx),
                PatchEditorEvent::Edit(text) => this.on_edit(text.clone(), cx),
                PatchEditorEvent::EditFailed(message) => this.on_edit_failed(message.clone(), cx),
            }));
        self.patch_editor = Some(editor);
    }

    /// Reads one side of the file `data` is of in full: an unstaged file's new side is on disk.
    fn load_file(&self, data: &FilePatch) -> LoadFile {
        let id = self.view.repository_id().clone();
        let path = data.file.path.clone();
        let unstaged = *self.view.source() == DiffSource::Unstaged;
        let read = self.editing.io().read;
        Rc::new(move |side, oid, cx| {
            if unstaged && side == Side::Additions {
                let reading = read(&id, &path, cx);
                cx.background_spawn(async move {
                    reading.await.map(|file| LoadedFile::WorkingTree {
                        contents: file.contents,
                        version: file.version,
                    })
                })
            } else {
                let reading = fetch_blob(&id, oid, cx);
                cx.background_spawn(async move { reading.await.map(LoadedFile::Blob) })
            }
        })
    }

    fn on_viewer(&mut self, event: &PatchViewerEvent, cx: &mut Context<Self>) {
        match event {
            PatchViewerEvent::Shown { file_key } => {
                if let Some(data) = self.viewer_patch.clone()
                    && data.key == *file_key
                {
                    self.view.patch_shown(&data, cx);
                    self.on_shown_for_editing(cx);
                }
            }
            PatchViewerEvent::LoadingFiles(loading) => self.view.set_loading_files(*loading, cx),
            PatchViewerEvent::FilesError(error) => self.view.set_files_error(error.clone()),
            PatchViewerEvent::Failed { file_key, message } => {
                self.viewer_error = Some((file_key.clone(), message.clone()));
            }
        }
        cx.notify();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        // The conflicts' keys, wherever the focus is in the view but in the editor.
        if self.on_conflict_key(event, window, cx) {
            cx.stop_propagation();
            return;
        }
        let keystroke = &event.keystroke;
        let modifiers = &keystroke.modifiers;
        if keystroke.key != "escape"
            || modifiers.alt
            || modifiers.control
            || modifiers.platform
            || modifiers.shift
        {
            return;
        }
        cx.stop_propagation();
        cx.emit(FileDiffEvent::Close);
    }

    fn open(&mut self, source: DiffSource, file: ChangedFile, cx: &mut Context<Self>) {
        cx.emit(FileDiffEvent::Open { source, file });
    }
}

impl Render for FileDiffView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync(window, cx);
        let palette = *cx.palette();
        let key = self.view.file_key();
        let staging_error = self.staging.error_for(&key).map(SharedString::from);
        let files_error = self.view.files_error().map(SharedString::from);
        v_flex()
            .id("file-diff-view")
            .key_context("FileDiffView")
            .track_focus(&self.focus)
            .size_full()
            .min_h_0()
            .min_w_0()
            .bg(palette.bg)
            .capture_key_down(cx.listener(Self::capture_escape))
            .on_key_down(cx.listener(Self::on_key_down))
            .child(self.render_header(window, cx))
            .children(self.render_editing_banner(cx))
            .when_some(staging_error, |this, message| {
                this.child(Notice::new(message).alert().icon(IconName::TriangleAlert))
            })
            .when(self.view.conflicted(), |this| {
                this.children(self.render_conflict_banner(cx))
            })
            .when_some(files_error, |this, message| {
                this.child(Notice::new(message))
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(self.render_body(cx)),
            )
    }
}

impl FileDiffView {
    /// The bar over the changes: the file on show, its line counts, and what can be done with it,
    /// from staging it whole to stepping to the files next to it. Its name stays while the next
    /// file's changes load.
    fn render_header(&mut self, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let palette = *cx.palette();
        let shown = self.view.shown();
        let file = &shown.file;
        let (folder, name) = match file.path.rfind('/') {
            Some(slash) => (&file.path[..=slash], &file.path[slash + 1..]),
            None => ("", file.path.as_str()),
        };
        let source = self.view.source().clone();
        let label = source_label(&source);
        let busy = self.view.busy().read(cx).is_on();
        let conflict = self.view.conflicted();
        let whole_file = if conflict { None } else { self.whole_file() };
        let conflict_controls = conflict.then(|| self.render_conflict_controls(cx));
        let editable = self.view.editable_patch() || self.is_conflict_editable();
        let editing_controls =
            (source == DiffSource::Unstaged).then(|| self.render_editing_controls(editable, cx));
        let staging_disabled = self.staging_disabled(cx);
        let previous = self.view.previous();
        let next = self.view.next();
        let style = DiffStyle::current(cx);
        let divider = || {
            div()
                .mx_1p5()
                .h_4()
                .w(px(1.))
                .flex_none()
                .bg(palette.border)
        };

        h_flex()
            .h(rems(2.8))
            .flex_none()
            .gap_2()
            .border_b_1()
            .border_color(palette.border)
            .pl_1p5()
            .pr_2()
            .child(
                IconButton::new(
                    "close-file",
                    IconName::ArrowLeft,
                    "Back to the history (Esc)",
                )
                .on_click(cx.listener(|_, _, _, cx| cx.emit(FileDiffEvent::Close))),
            )
            .child(FileStatusBadge::new("file-status", file.status))
            .child(
                h_flex()
                    .min_w_0()
                    .gap_2()
                    .child(
                        h_flex()
                            .min_w_0()
                            .text_size(rems(0.833))
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_color(palette.faint)
                                    .child(folder.to_owned()),
                            )
                            // Cut short too, rather than run into the label, in a narrow pane.
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(name.to_owned()),
                            ),
                    )
                    .when_some(file.orig_path.clone(), |this, orig_path| {
                        this.child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_size(rems(0.733))
                                .text_color(palette.faint)
                                .child(format!("from {orig_path}")),
                        )
                    })
                    .when_some(label, |this, label| {
                        this.child(
                            div()
                                .flex_none()
                                .text_size(rems(0.733))
                                .text_color(palette.faint)
                                .child(label),
                        )
                    }),
            )
            .when_some(shown.counts, |this, counts| {
                this.child(
                    div()
                        .flex_none()
                        .text_size(rems(0.733))
                        .child(LineStats::new(counts.additions, counts.deletions)),
                )
            })
            .when(busy, |this| this.child(spinner()))
            .child(
                h_flex()
                    .ml_auto()
                    .flex_none()
                    .gap_0p5()
                    .when_some(whole_file, |this, action| {
                        let (icon, label) = match action {
                            StageAction::Stage => (IconName::Plus, "Stage file"),
                            StageAction::Unstage => (IconName::Minus, "Unstage file"),
                        };
                        this.child(
                            action_button("stage-file")
                                .icon(Icon::new(icon))
                                .label(label)
                                .disabled(staging_disabled)
                                .on_click(cx.listener(|this, _, _, cx| this.stage_whole(cx))),
                        )
                        .child(divider())
                    })
                    .when_some(conflict_controls, |this, controls| {
                        this.children(controls).child(divider())
                    })
                    .when_some(editing_controls, |this, controls| {
                        this.children(controls).child(divider())
                    })
                    .child({
                        let previous = previous.clone();
                        let source = source.clone();
                        IconButton::new("previous-file", IconName::ChevronUp, "Previous file")
                            .disabled(previous.is_none())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(file) = previous.clone() {
                                    this.open(source.clone(), file, cx);
                                }
                            }))
                    })
                    .child({
                        let next = next.clone();
                        let source = source.clone();
                        IconButton::new("next-file", IconName::ChevronDown, "Next file")
                            .disabled(next.is_none())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(file) = next.clone() {
                                    this.open(source.clone(), file, cx);
                                }
                            }))
                    })
                    // Conflicts are shown one way only, ours above theirs.
                    .when(!conflict, |this| {
                        this.child(divider())
                            .child(
                                IconButton::new("unified", IconName::Rows2, "Unified")
                                    .active(style == DiffStyle::Unified)
                                    .on_click(|_, _, cx| DiffStyle::Unified.apply(cx)),
                            )
                            .child(
                                IconButton::new("split", IconName::Columns2, "Side by side")
                                    .active(style == DiffStyle::Split)
                                    .on_click(|_, _, cx| DiffStyle::Split.apply(cx)),
                            )
                    }),
            )
            .into_any_element()
    }

    fn render_body(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let view = &self.view;
        match self.body() {
            Body::NoLongerChanged => self.render_no_longer_changed(cx),
            Body::Conflict => self.render_conflict_body(cx),
            Body::Nested => EmptyState::new("nested", IconName::FolderGit2, "Another repository")
                .message(
                    "This folder is a git repository of its own, so its files aren't listed here.",
                )
                .fill()
                .into_any_element(),
            Body::Binary => EmptyState::new("binary", IconName::FileCheck, "Binary file")
                .message("Its changes can't be shown as text.")
                .fill()
                .into_any_element(),
            Body::NoChangedLines => {
                EmptyState::new("no-changed-lines", IconName::FileCheck, "No changed lines")
                    .message(unchanged_reason(&view.file()))
                    .fill()
                    .into_any_element()
            }
            Body::Large(lines) => {
                EmptyState::new("large", IconName::FileExclamationPoint, "Large change")
                    .message(format!(
                        "{} lines changed. Showing them may take a moment.",
                        thousands(lines)
                    ))
                    .fill()
                    .child(
                        Button::new("show-large")
                            .small()
                            .label("Show anyway")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.view.show_large(cx);
                                cx.notify();
                            })),
                    )
                    .into_any_element()
            }
            Body::Error(message) => EmptyState::new(
                "diff-error",
                IconName::TriangleAlert,
                "Couldn't load the changes",
            )
            .message(message)
            .error()
            .fill()
            .into_any_element(),
            Body::Loading => LoadingState::new("diff-loading", "Loading changes…")
                .fill()
                .into_any_element(),
            // The file being edited, in the diff's place, once it's read.
            Body::Patch
                if let Some(editor) = self
                    .patch_editor
                    .as_ref()
                    .filter(|editor| editor.read(cx).editor().is_some()) =>
            {
                editor.clone().into_any_element()
            }
            Body::Patch => match &self.viewer {
                Some(viewer) => viewer.clone().into_any_element(),
                None => LoadingState::new("diff-loading", "Loading changes…")
                    .fill()
                    .into_any_element(),
            },
        }
    }

    /// Says that an uncommitted file has no changes left on its side, e.g. once it's been staged,
    /// with a way to the other side's if it has some there.
    fn render_no_longer_changed(&self, cx: &mut Context<Self>) -> AnyElement {
        let staged = *self.view.source() == DiffSource::Staged;
        let path = self.view.file().path;
        let (other, files) = if staged {
            (DiffSource::Unstaged, self.view.lists().unstaged())
        } else {
            (DiffSource::Staged, self.view.lists().staged())
        };
        let found = files.iter().find(|file| file.path == path).cloned();
        EmptyState::new(
            "no-longer-changed",
            IconName::FileCheck,
            if staged {
                "No staged changes"
            } else {
                "No unstaged changes"
            },
        )
        .message(if staged {
            "This file's changes were unstaged or committed."
        } else {
            "This file's changes were staged or undone."
        })
        .fill()
        .when_some(found, |this, file| {
            let label = if other == DiffSource::Staged {
                "Show staged changes"
            } else {
                "Show unstaged changes"
            };
            this.child(
                Button::new("show-other-side")
                    .small()
                    .label(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open(other.clone(), file.clone(), cx)
                    })),
            )
        })
        .into_any_element()
    }
}

/// What the header says the changes are, besides the file: which side of the uncommitted ones.
fn source_label(source: &DiffSource) -> Option<&'static str> {
    match source {
        DiffSource::Unstaged => Some("Unstaged"),
        DiffSource::Staged => Some("Staged"),
        DiffSource::Commit { .. } | DiffSource::Stash { .. } => None,
    }
}

/// `n` with its thousands separated, as `toLocaleString` writes it in English.
fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (ix, digit) in digits.chars().enumerate() {
        if ix > 0 && (digits.len() - ix).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// Says changes are loading, once that's taken a moment.
fn spinner() -> impl IntoElement {
    Icon::new(IconName::LoaderCircle).size_3().with_animation(
        ElementId::from("loading-changes"),
        Animation::new(Duration::from_secs(1)).repeat(),
        |icon, delta| icon.transform(Transformation::rotate(percentage(delta))),
    )
}

#[cfg(test)]
mod tests;
