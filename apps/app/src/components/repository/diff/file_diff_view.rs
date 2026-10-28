//! A file's changes, in place of the history. Ported from `file-diff-view.tsx`.

use std::rc::Rc;
use std::time::Duration;

use gitto_git::FileStatus;
use gitto_git::features::diff::schema::ChangedFile;
use gitto_git::input::RepositoryId;
use gitto_ui::diff::diff_metadata::Side;
use gitto_ui::git::changes::{counts_of, has_patch, total_lines, unchanged_reason};
use gpui_kit::component::button::Button;
use gpui_kit::component::{Disableable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    Animation, AnimationExt as _, AnyElement, AnyWindowHandle, App, Context, ElementId, Entity,
    EventEmitter, FocusHandle, Focusable, FontWeight, KeyDownEvent, SharedString, Subscription,
    Task, Transformation, Window, div, percentage, px, rems,
};

use super::conflict_viewer::{ConflictViewer, SaveFile};
use super::file_staging::{FileStaging, StageFileMutation, StageLinesMutation};
use super::patch_viewer::{PatchProps, PatchViewer, PatchViewerEvent};
use super::shown_file::{ShownFile, patch_cache_key};
use super::viewer_runtime::{LoadFile, LoadedFile, prepare_patch};
use crate::components::ui::{
    EmptyState, FileStatusBadge, Icon, IconButton, IconName, LineStats, LoadingState, Notice,
};
use crate::data::QueryClient;
use crate::data::conflicts::conflict_options;
use crate::data::diff::fetch_blob;
use crate::data::file_diff::{
    FilePatch, fetch_file_patch, fetch_working_tree_file, save_working_tree_file,
};
use crate::data::staging::StageAction;
use crate::git::diff_source::DiffSource;
use crate::state::diff_style::{DiffStyle, DiffStyleSetting};
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
        let options = conflict_options(repository_id, &file.path, cx);
        QueryClient::fetch_query(options, cx).detach();
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
/// time (see `file_staging`), one at a time. Once that leaves it without changes on its side, the
/// next file in its list opens. A conflicted one shows its conflicts instead, to resolve (see
/// `conflict_viewer`).
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
    conflict: Option<Entity<ConflictViewer>>,
    /// The file on show whose neighbours were loaded ahead.
    prefetched_for: Option<String>,
    /// What had the focus when the changes opened, to give it back once they close.
    opener: Option<FocusHandle>,
    window: AnyWindowHandle,
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
        let view = ShownFile::new(repository_id, source, file, cx);
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
            conflict: None,
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

    /// Resolves to whether the file may be closed, or another opened, once its edits are saved
    /// (TS's `onGuard`). Editing a file is the file editing's, which isn't here yet: nothing to
    /// wait for.
    pub fn may_leave(&mut self, _: &mut Context<Self>) -> Task<bool> {
        Task::ready(true)
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
            self.view.patch_left(cx);
        }
        if body == Body::Conflict {
            self.show_conflict(window, cx);
        } else {
            self.conflict = None;
        }
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

    /// Reads one side of the file `data` is of in full: an unstaged file's new side is on disk.
    fn load_file(&self, data: &FilePatch) -> LoadFile {
        let id = self.view.repository_id().clone();
        let path = data.file.path.clone();
        let unstaged = *self.view.source() == DiffSource::Unstaged;
        Rc::new(move |side, oid, cx| {
            if unstaged && side == Side::Additions {
                let reading = fetch_working_tree_file(&id, &path, cx);
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

    /// Shows the conflicted file's conflicts, in the conflicts' viewer.
    fn show_conflict(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let key = self.view.file_key();
        if self
            .conflict
            .as_ref()
            .is_some_and(|conflict| *conflict.read(cx).file_key() == key)
        {
            return;
        }
        let id = self.view.repository_id().clone();
        let path = self.view.file().path;
        let saved_path = path.clone();
        let save: SaveFile = Rc::new(move |contents, version, cx| {
            save_working_tree_file(&id, &saved_path, contents, version, false, cx)
        });
        self.conflict = Some(cx.new(|cx| ConflictViewer::new(key, path, save, window, cx)));
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
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
            .on_key_down(cx.listener(Self::on_key_down))
            .child(self.render_header(window, cx))
            .when_some(staging_error, |this, message| {
                this.child(Notice::new(message).alert().icon(IconName::TriangleAlert))
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
        let conflict = self.view.conflicted() && self.conflict.is_some();
        let whole_file = if conflict { None } else { self.whole_file() };
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
                            .child(
                                div()
                                    .flex_none()
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
                            Button::new("stage-file")
                                .small()
                                .icon(Icon::new(icon))
                                .label(label)
                                .disabled(staging_disabled)
                                .on_click(cx.listener(|this, _, _, cx| this.stage_whole(cx))),
                        )
                        .child(divider())
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
            Body::Conflict => match &self.conflict {
                Some(conflict) => conflict.clone().into_any_element(),
                None => LoadingState::new("conflict-loading", "Loading changes…")
                    .fill()
                    .into_any_element(),
            },
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
mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    use futures::FutureExt as _;
    use futures::channel::oneshot;
    use gitto_git::features::staging::patch_version::patch_version;
    use gitto_git::features::staging::schema::{LineRange, LineSelection};
    use gitto_git::features::status::schema::{Head, StatusCounts, Uncommitted, WorkingTreeFiles};
    use gitto_ui::git::changes::staging_paths;
    use gpui_kit::component::WindowExt as _;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{AppContext as _, TestAppContext};

    use super::super::line_picker::LineStaging;
    use super::super::viewer_runtime::ViewerRuntime;
    use super::*;
    use crate::data::diff::ChangedFiles;
    use crate::data::file_diff::file_patch_key;
    use crate::data::staging::{FileToStage, LinesToStage};
    use crate::data::{Mutation, Opaque, QueryError, QueryOptions, keys};

    fn id() -> RepositoryId {
        RepositoryId::parse("01890a5d-ac96-774b-bcce-b302099a8057").unwrap()
    }

    fn file(path: &str, additions: Option<u64>, deletions: Option<u64>) -> ChangedFile {
        ChangedFile {
            path: path.into(),
            status: FileStatus::Modified,
            orig_path: None,
            additions,
            deletions,
        }
    }

    fn counted(path: &str) -> ChangedFile {
        file(path, Some(1), Some(0))
    }

    fn with_status(file: ChangedFile, status: FileStatus) -> ChangedFile {
        ChangedFile { status, ..file }
    }

    const SHA: &str = "a1a1a1a";

    fn commit() -> DiffSource {
        DiffSource::Commit { sha: SHA.into() }
    }

    fn files() -> Vec<ChangedFile> {
        vec![counted("a.txt"), counted("b.txt"), counted("c.txt")]
    }

    /// A patch of `path` with one line changed to `line`, as git would give it.
    fn changed_line(path: &str, line: &str) -> String {
        format!(
            "diff --git a/{path} b/{path}\nindex 1111111..2222222 100644\n--- a/{path}\n+++ b/{path}\n@@ -1 +1 @@\n-old\n+{line}\n"
        )
    }

    /// Gives `file` of `source` the patch `patch`, as if it was loaded.
    fn set_patch(source: &DiffSource, file: &ChangedFile, patch: &str, cx: &mut TestAppContext) {
        cx.update(|cx| set_patch_in(source, file, patch, cx));
    }

    fn set_patch_in(source: &DiffSource, file: &ChangedFile, patch: &str, cx: &mut App) {
        QueryClient::set_data(
            &file_patch_key(&id(), source, &file.path),
            FilePatch {
                key: source.file_key(&file.path),
                file: file.clone(),
                patch: patch.into(),
            },
            cx,
        )
    }

    /// Starts loading `file`'s patch in `source`, to be given by the sender returned: until then,
    /// it's loading.
    fn pending_patch(
        source: &DiffSource,
        file: &ChangedFile,
        cx: &mut TestAppContext,
    ) -> oneshot::Sender<FilePatch> {
        let (send, receive) = oneshot::channel::<FilePatch>();
        let receive = Arc::new(Mutex::new(Some(receive)));
        let options = QueryOptions::new(file_patch_key(&id(), source, &file.path), move |_, _| {
            let receive = receive.lock().unwrap().take();
            async move {
                match receive {
                    Some(receive) => receive.await.map_err(|_| QueryError::msg("Dropped.")),
                    None => futures::future::pending().await,
                }
            }
            .boxed()
        });
        cx.update(|cx| {
            let entry = QueryClient::entry::<FilePatch>(options.key(), cx);
            entry.update(cx, |entry, cx| {
                entry.set_options(&options);
                let _ = entry.fetch(cx);
            });
        });
        send
    }

    /// The commit's files, and each one's patch.
    fn commit_files(cx: &mut TestAppContext) {
        cx.update(|cx| {
            QueryClient::set_data(
                &keys::commit_files(&id(), SHA),
                ChangedFiles {
                    sha: SHA.into(),
                    files: Opaque::new(files()),
                },
                cx,
            )
        });
        patches(&commit(), &files(), "new", cx);
    }

    /// The uncommitted changes the status says there are.
    fn set_changes(
        staged: Vec<ChangedFile>,
        unstaged: Vec<ChangedFile>,
        uncounted: bool,
        cx: &mut TestAppContext,
    ) {
        let status = Uncommitted {
            head: Head::Branch {
                name: "main".into(),
                sha: "a1".into(),
            },
            upstream: None,
            ahead: 0,
            behind: 0,
            counts: StatusCounts {
                files: 0,
                staged: 0,
                unstaged: 0,
                conflicted: 0,
            },
            changes: WorkingTreeFiles {
                staged,
                unstaged,
                uncounted,
                marker_free: Vec::new(),
            },
            version: uuid::Uuid::new_v4().to_string(),
        };
        cx.update(|cx| QueryClient::set_data(&keys::status(&id()), Opaque::new(status), cx));
    }

    /// Gives each of `files` of `source` a patch changing its line to `<prefix> <path>`.
    fn patches(source: &DiffSource, files: &[ChangedFile], prefix: &str, cx: &mut TestAppContext) {
        for file in files {
            set_patch(
                source,
                file,
                &changed_line(&file.path, &format!("{prefix} {}", file.path)),
                cx,
            );
        }
    }

    type Respond<I, R> = Box<dyn FnMut(&I, &mut App) -> Task<Result<R, QueryError>>>;

    /// A mutation a test makes up: it records its inputs, and does what `respond` says.
    struct Fake<I: 'static, R: 'static> {
        mutation: Entity<Mutation<I, R>>,
        calls: Rc<RefCell<Vec<I>>>,
        respond: Rc<RefCell<Respond<I, R>>>,
    }

    impl<I: Clone + 'static, R: 'static> Fake<I, R> {
        fn new(cx: &mut TestAppContext) -> Self {
            let calls: Rc<RefCell<Vec<I>>> = Rc::default();
            let respond: Rc<RefCell<Respond<I, R>>> = Rc::new(RefCell::new(Box::new(|_, _| {
                Task::ready(Err(QueryError::msg("No reply set.")))
            })));
            let mutation = cx.update(|cx| {
                let calls = Rc::clone(&calls);
                let respond = Rc::clone(&respond);
                Mutation::new(
                    move |input: I, cx: &mut App| {
                        calls.borrow_mut().push(input.clone());
                        (respond.borrow_mut())(&input, cx)
                    },
                    cx,
                )
            });
            Fake {
                mutation,
                calls,
                respond,
            }
        }

        fn respond(
            &self,
            respond: impl FnMut(&I, &mut App) -> Task<Result<R, QueryError>> + 'static,
        ) {
            *self.respond.borrow_mut() = Box::new(respond);
        }

        fn calls(&self) -> Vec<I> {
            self.calls.borrow().clone()
        }
    }

    /// A file's changes, open, with what they asked of the page.
    struct Opened {
        window: AnyWindowHandle,
        view: Entity<FileDiffView>,
        events: Rc<RefCell<Vec<FileDiffEvent>>>,
        lines: Fake<LinesToStage, String>,
        whole: Fake<FileToStage, ()>,
    }

    impl Opened {
        /// The patch on show.
        fn shown(&self, cx: &mut TestAppContext) -> Option<String> {
            cx.update(|cx| {
                let viewer = self.view.read(cx).viewer()?.clone();
                viewer.read(cx).shown_patch().map(str::to_owned)
            })
        }

        /// The file the header names.
        fn header(&self, cx: &mut TestAppContext) -> String {
            cx.update(|cx| self.view.read(cx).view.shown().file.path)
        }

        fn body(&self, cx: &mut TestAppContext) -> Body {
            cx.update(|cx| self.view.read(cx).body())
        }

        fn staging(&self, cx: &mut TestAppContext) -> Option<LineStaging> {
            cx.update(|cx| self.view.read(cx).line_staging(cx))
        }

        fn events(&self) -> Vec<FileDiffEvent> {
            self.events.borrow().clone()
        }

        fn staging_error(&self, cx: &mut TestAppContext) -> Option<String> {
            cx.update(|cx| {
                let view = self.view.read(cx);
                view.staging
                    .error_for(&view.view.file_key())
                    .map(str::to_owned)
            })
        }

        fn set_file(&self, source: DiffSource, file: ChangedFile, cx: &mut TestAppContext) {
            self.view
                .update(cx, |view, cx| view.set_file(id(), source, file, cx));
            cx.run_until_parked();
        }

        fn click(&self, element: &'static str, cx: &mut TestAppContext) {
            cx.update_window(self.window, |_, window, cx| {
                window.render_frame(cx);
                window.click(element, cx);
            })
            .unwrap();
            cx.run_until_parked();
        }

        fn press(&self, key: &'static str, cx: &mut TestAppContext) {
            cx.update_window(self.window, |_, window, cx| window.press(key, cx))
                .unwrap();
            cx.run_until_parked();
        }

        fn is_enabled(&self, element: &'static str, cx: &mut TestAppContext) -> bool {
            cx.update_window(self.window, |_, window, cx| {
                window.render_frame(cx);
                !window.find(element).disabled().unwrap_or(false)
            })
            .unwrap()
        }

        fn has(&self, element: &'static str, cx: &mut TestAppContext) -> bool {
            cx.update_window(self.window, |_, window, cx| {
                window.render_frame(cx);
                window.try_find(element).is_some()
            })
            .unwrap()
        }

        /// Stages `lines` of `patch` as the viewer would; what it resolved to, once it has.
        fn stage(
            &self,
            patch: &str,
            lines: LineSelection,
            cx: &mut TestAppContext,
        ) -> Rc<RefCell<Option<Result<(), QueryError>>>> {
            let staging = self.staging(cx).expect("lines can be staged");
            let result = Rc::new(RefCell::new(None));
            let done = Rc::clone(&result);
            let patch = patch.to_owned();
            cx.update(|cx| {
                let staged = (staging.stage)(patch, lines, cx);
                cx.spawn(async move |_| *done.borrow_mut() = Some(staged.await))
                    .detach();
            });
            cx.run_until_parked();
            result
        }
    }

    fn open(source: DiffSource, shown: ChangedFile, cx: &mut TestAppContext) -> Opened {
        let lines = Fake::new(cx);
        let whole = Fake::new(cx);
        let (stage_lines, stage_file) = (lines.mutation.clone(), whole.mutation.clone());
        let (window, view) = crate::test_support::open(cx, |window, cx| {
            FileDiffView::with_mutations(id(), source, shown, stage_lines, stage_file, window, cx)
        });
        let events: Rc<RefCell<Vec<FileDiffEvent>>> = Rc::default();
        let recorded = Rc::clone(&events);
        cx.update(|cx| {
            cx.subscribe(&view, move |_, event: &FileDiffEvent, _| {
                recorded.borrow_mut().push(event.clone())
            })
            .detach()
        });
        cx.run_until_parked();
        Opened {
            window,
            view,
            events,
            lines,
            whole,
        }
    }

    fn init(cx: &mut TestAppContext) -> tempfile::TempDir {
        crate::test_support::init(cx, |services| services)
    }

    fn lines() -> LineSelection {
        LineSelection {
            deletions: vec![LineRange::new(1, 1).unwrap()],
            additions: vec![LineRange::new(1, 1).unwrap()],
        }
    }

    fn opened_with(source: DiffSource, file: ChangedFile) -> FileDiffEvent {
        FileDiffEvent::Open { source, file }
    }

    // A file's changes.

    #[gpui_kit::test]
    fn shows_the_files_patch_a_step_away_from_the_commits_other_files(cx: &mut TestAppContext) {
        let _dir = init(cx);
        commit_files(cx);
        let opened = open(commit(), counted("b.txt"), cx);

        assert_eq!(opened.shown(cx), Some(changed_line("b.txt", "new b.txt")));
        opened.click("next-file", cx);
        assert_eq!(opened.events(), [opened_with(commit(), counted("c.txt"))]);
        opened.click("previous-file", cx);
        assert_eq!(
            opened.events().last(),
            Some(&opened_with(commit(), counted("a.txt")))
        );
    }

    #[gpui_kit::test]
    fn keeps_the_last_file_on_show_header_and_all_until_the_next_ones_changes_are_in(
        cx: &mut TestAppContext,
    ) {
        let _dir = init(cx);
        cx.update(|cx| {
            QueryClient::set_data(
                &keys::commit_files(&id(), SHA),
                ChangedFiles {
                    sha: SHA.into(),
                    files: Opaque::new(files()),
                },
                cx,
            )
        });
        patches(&commit(), &[counted("a.txt"), counted("c.txt")], "new", cx);
        let next = pending_patch(&commit(), &counted("b.txt"), cx);
        let opened = open(commit(), counted("a.txt"), cx);
        assert_eq!(opened.shown(cx), Some(changed_line("a.txt", "new a.txt")));

        opened.set_file(commit(), counted("b.txt"), cx);
        assert_eq!(opened.shown(cx), Some(changed_line("a.txt", "new a.txt")));
        assert_eq!(opened.header(cx), "a.txt");

        next.send(FilePatch {
            key: commit().file_key("b.txt"),
            file: counted("b.txt"),
            patch: changed_line("b.txt", "later").into(),
        })
        .unwrap();
        cx.run_until_parked();
        assert_eq!(opened.shown(cx), Some(changed_line("b.txt", "later")));
        assert_eq!(opened.header(cx), "b.txt");
    }

    #[gpui_kit::test]
    fn loads_the_files_a_step_away_ahead_once_its_on_show(cx: &mut TestAppContext) {
        let _dir = init(cx);
        commit_files(cx);
        let opened = open(commit(), counted("b.txt"), cx);
        assert!(opened.shown(cx).is_some());
        // Parsed and highlighted, named as they'll be when they're opened.
        cx.update(|cx| {
            for path in ["a.txt", "c.txt"] {
                let key = format!("{}:0", commit().file_key(path));
                assert!(ViewerRuntime::has_highlights(&key, cx), "{path}");
            }
        });
    }

    #[gpui_kit::test]
    fn cant_step_past_the_first_file(cx: &mut TestAppContext) {
        let _dir = init(cx);
        commit_files(cx);
        let opened = open(commit(), counted("a.txt"), cx);

        assert!(opened.shown(cx).is_some());
        assert!(!opened.is_enabled("previous-file", cx));
        assert!(opened.is_enabled("next-file", cx));
    }

    #[gpui_kit::test]
    fn doesnt_load_a_binary_files_patch(cx: &mut TestAppContext) {
        let _dir = init(cx);
        commit_files(cx);
        let image = file("image.png", None, None);
        let opened = open(commit(), image.clone(), cx);

        assert_eq!(opened.body(cx), Body::Binary);
        let key = file_patch_key(&id(), &commit(), &image.path);
        let loading = cx.update(|cx| {
            QueryClient::get_entry::<FilePatch>(&key, cx)
                .is_some_and(|entry| entry.read(cx).is_fetching())
        });
        assert!(!loading);
    }

    #[gpui_kit::test]
    fn says_a_file_was_only_renamed(cx: &mut TestAppContext) {
        let _dir = init(cx);
        commit_files(cx);
        let renamed = ChangedFile {
            status: FileStatus::Renamed,
            orig_path: Some("old.txt".into()),
            ..file("new.txt", Some(0), Some(0))
        };
        let opened = open(commit(), renamed.clone(), cx);

        assert_eq!(opened.body(cx), Body::NoChangedLines);
        assert_eq!(
            unchanged_reason(&renamed),
            "Renamed from old.txt, with the same contents."
        );
    }

    #[gpui_kit::test]
    fn asks_before_loading_a_large_change(cx: &mut TestAppContext) {
        let _dir = init(cx);
        commit_files(cx);
        let big = file("big.lock", Some(30_000), Some(0));
        set_patch(&commit(), &big, &changed_line("big.lock", "big"), cx);
        let opened = open(commit(), big, cx);

        assert_eq!(opened.body(cx), Body::Large(30_000));
        opened.click("show-large", cx);
        assert_eq!(opened.shown(cx), Some(changed_line("big.lock", "big")));
    }

    #[gpui_kit::test]
    fn tries_the_next_file_afresh_after_one_couldnt_be_shown(cx: &mut TestAppContext) {
        let _dir = init(cx);
        commit_files(cx);
        let broken = counted("broken.txt");
        set_patch(&commit(), &broken, "patch of broken.txt", cx);
        let opened = open(commit(), broken, cx);

        assert_eq!(
            opened.body(cx),
            Body::Error("The patch has no file in it.".into())
        );
        opened.set_file(commit(), counted("b.txt"), cx);
        assert_eq!(opened.shown(cx), Some(changed_line("b.txt", "new b.txt")));
    }

    #[gpui_kit::test]
    fn says_an_empty_file_was_added(cx: &mut TestAppContext) {
        let _dir = init(cx);
        commit_files(cx);
        let added = with_status(file(".gitkeep", Some(0), Some(0)), FileStatus::Added);
        let opened = open(commit(), added.clone(), cx);

        assert_eq!(opened.body(cx), Body::NoChangedLines);
        assert_eq!(unchanged_reason(&added), "An empty file was added.");
    }

    #[gpui_kit::test]
    fn leaves_esc_to_a_dialog_that_closes_on_it(cx: &mut TestAppContext) {
        let _dir = init(cx);
        commit_files(cx);
        let opened = open(commit(), counted("a.txt"), cx);
        cx.update_window(opened.window, |_, window, cx| {
            window.open_dialog(cx, |dialog, _, _| dialog)
        })
        .unwrap();
        cx.run_until_parked();

        opened.press("escape", cx);
        assert_eq!(opened.events(), []);
        let open = cx
            .update_window(opened.window, |_, window, cx| window.has_active_dialog(cx))
            .unwrap();
        assert!(!open);
    }

    /// What opens a file's changes, to give the focus back to.
    struct Host {
        opener: FocusHandle,
        view: Option<Entity<FileDiffView>>,
        _subscription: Option<Subscription>,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .child(
                    div()
                        .id("opener")
                        .track_focus(&self.opener)
                        .child("Open a.txt"),
                )
                .children(self.view.clone())
        }
    }

    #[gpui_kit::test]
    fn gives_the_focus_back_to_what_opened_it_once_it_closes(cx: &mut TestAppContext) {
        let _dir = init(cx);
        commit_files(cx);
        let (window, host) = crate::test_support::open(cx, |_, cx| Host {
            opener: cx.focus_handle(),
            view: None,
            _subscription: None,
        });
        cx.update_window(window, |_, window, cx| {
            let opener = host.read(cx).opener.clone();
            window.focus(&opener, cx);
            host.update(cx, |host, cx| {
                let view = cx.new(|cx| {
                    let lines = Mutation::new(|_, _| Task::ready(Ok(String::new())), cx);
                    let whole = Mutation::new(|_, _| Task::ready(Ok(())), cx);
                    FileDiffView::with_mutations(
                        id(),
                        commit(),
                        counted("a.txt"),
                        lines,
                        whole,
                        window,
                        cx,
                    )
                });
                host._subscription = Some(cx.subscribe(&view, |host: &mut Host, _, event, cx| {
                    if *event == FileDiffEvent::Close {
                        host.view = None;
                        cx.notify();
                    }
                }));
                host.view = Some(view);
                cx.notify();
            });
        })
        .unwrap();
        cx.run_until_parked();

        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.click("close-file", cx);
        })
        .unwrap();
        cx.run_until_parked();
        let focused = cx
            .update_window(window, |_, window, cx| {
                host.read(cx).opener.is_focused(window)
            })
            .unwrap();
        assert!(host.read_with(cx, |host, _| host.view.is_none()));
        assert!(focused);
    }

    #[gpui_kit::test]
    fn goes_back_to_the_history_with_esc(cx: &mut TestAppContext) {
        let _dir = init(cx);
        commit_files(cx);
        let opened = open(commit(), counted("a.txt"), cx);

        assert!(opened.shown(cx).is_some());
        opened.press("escape", cx);
        assert_eq!(opened.events(), [FileDiffEvent::Close]);
    }

    // An uncommitted file's changes.

    #[gpui_kit::test]
    fn are_updated_in_place_when_they_change_on_disk(cx: &mut TestAppContext) {
        let _dir = init(cx);
        set_changes(vec![], vec![counted("a.txt")], false, cx);
        patches(&DiffSource::Unstaged, &[counted("a.txt")], "new", cx);
        let opened = open(DiffSource::Unstaged, counted("a.txt"), cx);
        assert_eq!(opened.shown(cx), Some(changed_line("a.txt", "new a.txt")));

        set_patch(
            &DiffSource::Unstaged,
            &counted("a.txt"),
            &changed_line("a.txt", "saved"),
            cx,
        );
        cx.run_until_parked();
        assert_eq!(opened.shown(cx), Some(changed_line("a.txt", "saved")));
    }

    #[gpui_kit::test]
    fn says_when_the_file_has_no_unstaged_changes_left_with_a_way_to_its_staged_ones(
        cx: &mut TestAppContext,
    ) {
        let _dir = init(cx);
        set_changes(vec![], vec![counted("a.txt")], false, cx);
        patches(&DiffSource::Unstaged, &[counted("a.txt")], "new", cx);
        let opened = open(DiffSource::Unstaged, counted("a.txt"), cx);
        assert!(opened.shown(cx).is_some());

        set_changes(vec![counted("a.txt")], vec![], false, cx);
        set_patch(&DiffSource::Unstaged, &counted("a.txt"), "", cx);
        cx.run_until_parked();
        assert_eq!(opened.body(cx), Body::NoLongerChanged);
        assert_eq!(opened.shown(cx), None);
        opened.click("show-other-side", cx);
        assert_eq!(
            opened.events(),
            [opened_with(DiffSource::Staged, counted("a.txt"))]
        );
    }

    #[gpui_kit::test]
    fn tells_from_the_patch_whether_an_untracked_file_is_binary(cx: &mut TestAppContext) {
        let _dir = init(cx);
        let image = with_status(file("image.png", None, None), FileStatus::Untracked);
        set_changes(vec![], vec![image.clone()], false, cx);
        set_patch(
            &DiffSource::Unstaged,
            &image,
            "diff --git a/image.png b/image.png\nnew file mode 100644\nindex 0000000..1111111\nBinary files /dev/null and b/image.png differ\n",
            cx,
        );
        let opened = open(DiffSource::Unstaged, image, cx);

        assert_eq!(opened.body(cx), Body::Binary);
    }

    #[gpui_kit::test]
    fn asks_before_showing_a_large_file_that_wasnt_counted(cx: &mut TestAppContext) {
        let _dir = init(cx);
        let log = file("huge.log", None, None);
        set_changes(vec![], vec![log.clone()], true, cx);
        let lines = "+line\n".repeat(25_000);
        set_patch(
            &DiffSource::Unstaged,
            &log,
            &format!(
                "diff --git a/huge.log b/huge.log\nindex 1..2 100644\n@@ -0,0 +1,25000 @@\n{lines}"
            ),
            cx,
        );
        let opened = open(DiffSource::Unstaged, log, cx);

        assert_eq!(opened.body(cx), Body::Large(25_000));
        assert_eq!(thousands(25_000), "25,000");
    }

    #[gpui_kit::test]
    fn stays_on_the_page_while_theyre_refetched_so_the_view_keeps_its_scroll_position(
        cx: &mut TestAppContext,
    ) {
        let _dir = init(cx);
        set_changes(vec![], vec![counted("a.txt")], false, cx);
        patches(&DiffSource::Unstaged, &[counted("a.txt")], "new", cx);
        let opened = open(DiffSource::Unstaged, counted("a.txt"), cx);
        let viewer = cx
            .update(|cx| opened.view.read(cx).viewer().cloned())
            .unwrap();

        set_changes(vec![], vec![file("a.txt", Some(2), Some(1))], false, cx);
        set_patch(
            &DiffSource::Unstaged,
            &counted("a.txt"),
            &changed_line("a.txt", "saved"),
            cx,
        );
        cx.run_until_parked();
        assert_eq!(opened.shown(cx), Some(changed_line("a.txt", "saved")));
        let still = cx
            .update(|cx| opened.view.read(cx).viewer().cloned())
            .unwrap();
        assert_eq!(still.entity_id(), viewer.entity_id());
    }

    #[gpui_kit::test]
    fn doesnt_step_from_where_another_file_was_for_one_that_isnt_in_its_list(
        cx: &mut TestAppContext,
    ) {
        let _dir = init(cx);
        let unstaged = vec![counted("a.txt"), counted("b.txt"), counted("c.txt")];
        set_changes(vec![counted("d.txt")], unstaged.clone(), false, cx);
        patches(&DiffSource::Unstaged, &unstaged, "new", cx);
        patches(
            &DiffSource::Staged,
            &[counted("d.txt"), counted("a.txt")],
            "staged",
            cx,
        );
        let opened = open(DiffSource::Unstaged, counted("b.txt"), cx);
        assert!(opened.is_enabled("next-file", cx));

        // Opened from the staged list as it was being unstaged.
        opened.set_file(DiffSource::Staged, counted("a.txt"), cx);
        assert_eq!(opened.body(cx), Body::NoLongerChanged);
        assert!(!opened.is_enabled("previous-file", cx));
        assert!(!opened.is_enabled("next-file", cx));
    }

    #[gpui_kit::test]
    fn doesnt_load_a_file_without_line_counts_ahead_which_can_be_of_any_size(
        cx: &mut TestAppContext,
    ) {
        let _dir = init(cx);
        let untracked = |path| with_status(file(path, None, None), FileStatus::Untracked);
        let unstaged = vec![counted("a.txt"), untracked("b.log"), untracked("c.log")];
        set_changes(vec![], unstaged.clone(), false, cx);
        patches(&DiffSource::Unstaged, &unstaged, "new", cx);
        let opened = open(DiffSource::Unstaged, untracked("b.log"), cx);

        assert_eq!(opened.shown(cx), Some(changed_line("b.log", "new b.log")));
        cx.update(|cx| {
            let key = |path: &str| {
                let patch = changed_line(path, &format!("new {path}"));
                format!(
                    "{}:{}:0",
                    DiffSource::Unstaged.file_key(path),
                    patch_version(&patch)
                )
            };
            assert!(ViewerRuntime::has_highlights(&key("a.txt"), cx));
            assert!(!ViewerRuntime::has_highlights(&key("c.log"), cx));
        });
    }

    // Staging lines.

    /// What the data layer does once lines are staged: the patch they leave takes the place of
    /// the one they were picked from, if it has changes left.
    fn leaves(
        source: DiffSource,
        file: ChangedFile,
        left: String,
    ) -> impl FnMut(&LinesToStage, &mut App) -> Task<Result<String, QueryError>> + 'static {
        move |_, cx| {
            if gitto_ui::git::patch::has_hunks(&left) {
                set_patch_in(&source, &file, &left, cx);
            }
            Task::ready(Ok(left.clone()))
        }
    }

    #[gpui_kit::test]
    fn stages_the_lines_picked_from_the_patch_on_show_and_shows_the_patch_they_leave(
        cx: &mut TestAppContext,
    ) {
        let _dir = init(cx);
        let unstaged = vec![counted("a.txt"), counted("b.txt")];
        set_changes(vec![], unstaged.clone(), false, cx);
        patches(&DiffSource::Unstaged, &unstaged, "new", cx);
        let opened = open(DiffSource::Unstaged, counted("a.txt"), cx);
        opened.lines.respond(leaves(
            DiffSource::Unstaged,
            counted("a.txt"),
            changed_line("a.txt", "left"),
        ));

        assert_eq!(opened.staging(cx).unwrap().action, StageAction::Stage);
        let from = changed_line("a.txt", "new a.txt");
        let result = opened.stage(&from, lines(), cx);
        assert!(matches!(*result.borrow(), Some(Ok(()))));
        let calls = opened.lines.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].file, counted("a.txt"));
        assert_eq!(calls[0].action, StageAction::Stage);
        assert_eq!(calls[0].version, patch_version(&from));
        assert_eq!(calls[0].lines, lines());
        assert_eq!(opened.shown(cx), Some(changed_line("a.txt", "left")));
    }

    #[gpui_kit::test]
    fn stages_lines_of_an_untracked_file_as_such(cx: &mut TestAppContext) {
        let _dir = init(cx);
        let notes = with_status(file("notes.txt", None, None), FileStatus::Untracked);
        set_changes(vec![], vec![notes.clone()], false, cx);
        patches(
            &DiffSource::Unstaged,
            std::slice::from_ref(&notes),
            "new",
            cx,
        );
        let opened = open(DiffSource::Unstaged, notes.clone(), cx);
        opened.lines.respond(leaves(
            DiffSource::Unstaged,
            notes,
            changed_line("notes.txt", "rest"),
        ));

        opened.stage("the patch", lines(), cx);
        assert_eq!(opened.lines.calls()[0].file.status, FileStatus::Untracked);
    }

    #[gpui_kit::test]
    fn unstages_lines_picked_from_the_staged_changes(cx: &mut TestAppContext) {
        let _dir = init(cx);
        set_changes(vec![counted("a.txt")], vec![], false, cx);
        patches(&DiffSource::Staged, &[counted("a.txt")], "staged", cx);
        let opened = open(DiffSource::Staged, counted("a.txt"), cx);
        assert_eq!(
            opened.shown(cx),
            Some(changed_line("a.txt", "staged a.txt"))
        );
        opened.lines.respond(leaves(
            DiffSource::Staged,
            counted("a.txt"),
            changed_line("a.txt", "still staged"),
        ));

        assert_eq!(opened.staging(cx).unwrap().action, StageAction::Unstage);
        opened.stage("the patch", lines(), cx);
        let calls = opened.lines.calls();
        assert_eq!(calls[0].action, StageAction::Unstage);
        assert_eq!(calls[0].version, patch_version("the patch"));
        assert_eq!(
            opened.shown(cx),
            Some(changed_line("a.txt", "still staged"))
        );
    }

    #[gpui_kit::test]
    fn is_busy_until_the_patch_the_lines_leave_is_on_show(cx: &mut TestAppContext) {
        let _dir = init(cx);
        set_changes(vec![], vec![counted("a.txt")], false, cx);
        patches(&DiffSource::Unstaged, &[counted("a.txt")], "new", cx);
        let opened = open(DiffSource::Unstaged, counted("a.txt"), cx);
        let replies: Rc<RefCell<VecDeque<oneshot::Sender<String>>>> = Rc::default();
        let senders = Rc::clone(&replies);
        opened.lines.respond(move |_, cx| {
            let (send, receive) = oneshot::channel();
            senders.borrow_mut().push_back(send);
            cx.background_spawn(
                async move { receive.await.map_err(|_| QueryError::msg("Dropped.")) },
            )
        });

        assert!(!opened.staging(cx).unwrap().busy);
        let from = changed_line("a.txt", "new a.txt");
        let result = opened.stage(&from, lines(), cx);
        assert!(opened.staging(cx).unwrap().busy);
        let left = changed_line("a.txt", "left");
        set_patch(&DiffSource::Unstaged, &counted("a.txt"), &left, cx);
        replies
            .borrow_mut()
            .pop_front()
            .unwrap()
            .send(left)
            .unwrap();
        cx.run_until_parked();
        assert!(matches!(*result.borrow(), Some(Ok(()))));
        assert_eq!(opened.shown(cx), Some(changed_line("a.txt", "left")));
        assert!(!opened.staging(cx).unwrap().busy);
    }

    #[gpui_kit::test]
    fn opens_the_next_file_once_the_last_lines_are_staged_this_ones_staying_on_show_till_then(
        cx: &mut TestAppContext,
    ) {
        let _dir = init(cx);
        let unstaged = vec![counted("a.txt"), counted("b.txt")];
        set_changes(vec![], unstaged.clone(), false, cx);
        patches(&DiffSource::Unstaged, &unstaged, "new", cx);
        let opened = open(DiffSource::Unstaged, counted("a.txt"), cx);
        opened.lines.respond(leaves(
            DiffSource::Unstaged,
            counted("a.txt"),
            String::new(),
        ));

        opened.stage(&changed_line("a.txt", "new a.txt"), lines(), cx);
        assert_eq!(
            opened.events(),
            [opened_with(DiffSource::Unstaged, counted("b.txt"))]
        );
        assert_ne!(opened.body(cx), Body::NoLongerChanged);
        assert_eq!(opened.shown(cx), Some(changed_line("a.txt", "new a.txt")));
    }

    #[gpui_kit::test]
    fn opens_the_one_before_once_the_last_files_last_lines_are_staged(cx: &mut TestAppContext) {
        let _dir = init(cx);
        let unstaged = vec![counted("a.txt"), counted("b.txt")];
        set_changes(vec![], unstaged.clone(), false, cx);
        patches(&DiffSource::Unstaged, &unstaged, "new", cx);
        let opened = open(DiffSource::Unstaged, counted("b.txt"), cx);
        // Only its mode change is left.
        opened.lines.respond(leaves(
            DiffSource::Unstaged,
            counted("b.txt"),
            "diff --git a/b.txt b/b.txt\nold mode 100644\nnew mode 100755\n".into(),
        ));

        opened.stage(&changed_line("b.txt", "new b.txt"), lines(), cx);
        assert_eq!(
            opened.events(),
            [opened_with(DiffSource::Unstaged, counted("a.txt"))]
        );
    }

    #[gpui_kit::test]
    fn says_why_lines_couldnt_be_staged_and_shows_the_changes_as_they_are_now(
        cx: &mut TestAppContext,
    ) {
        let _dir = init(cx);
        set_changes(vec![], vec![counted("a.txt")], false, cx);
        patches(&DiffSource::Unstaged, &[counted("a.txt")], "new", cx);
        let opened = open(DiffSource::Unstaged, counted("a.txt"), cx);
        let message = "The file changed since its changes were shown, so nothing was staged.";
        opened.lines.respond(move |_, cx| {
            // The changes are reloaded, as they are now.
            set_patch_in(
                &DiffSource::Unstaged,
                &counted("a.txt"),
                &changed_line("a.txt", "saved"),
                cx,
            );
            Task::ready(Err(QueryError::msg(message)))
        });

        let result = opened.stage(&changed_line("a.txt", "new a.txt"), lines(), cx);
        let failed = result
            .borrow()
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap_err()
            .message();
        assert_eq!(failed, message);
        assert_eq!(opened.staging_error(cx).as_deref(), Some(message));
        assert_eq!(opened.shown(cx), Some(changed_line("a.txt", "saved")));
    }

    #[gpui_kit::test]
    fn doesnt_pick_lines_of_a_commits_changes_nor_of_a_links(cx: &mut TestAppContext) {
        let _dir = init(cx);
        commit_files(cx);
        let opened = open(commit(), counted("a.txt"), cx);
        assert!(opened.shown(cx).is_some());
        assert!(opened.staging(cx).is_none());

        set_changes(vec![], vec![counted("link")], false, cx);
        set_patch(
            &DiffSource::Unstaged,
            &counted("link"),
            "diff --git a/link b/link\nindex 1..2 120000\n--- a/link\n+++ b/link\n@@ -1 +1 @@\n-a\n\\ No newline at end of file\n+b\n\\ No newline at end of file\n",
            cx,
        );
        let link = open(DiffSource::Unstaged, counted("link"), cx);
        assert!(link.shown(cx).is_some());
        assert!(link.staging(cx).is_none());
    }

    // Staging the whole file.

    fn staged_whole() -> impl FnMut(&FileToStage, &mut App) -> Task<Result<(), QueryError>> {
        |_, _| Task::ready(Ok(()))
    }

    #[gpui_kit::test]
    fn stages_it_then_opens_the_next_file_in_the_list(cx: &mut TestAppContext) {
        let _dir = init(cx);
        let unstaged = vec![counted("a.txt"), counted("b.txt")];
        set_changes(vec![], unstaged.clone(), false, cx);
        patches(&DiffSource::Unstaged, &unstaged, "new", cx);
        let opened = open(DiffSource::Unstaged, counted("a.txt"), cx);
        opened.whole.respond(staged_whole());

        opened.click("stage-file", cx);
        let calls = opened.whole.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].action, StageAction::Stage);
        assert_eq!(staging_paths(&[calls[0].file.clone()]), ["a.txt"]);
        assert_eq!(
            opened.events(),
            [opened_with(DiffSource::Unstaged, counted("b.txt"))]
        );
    }

    #[gpui_kit::test]
    fn stages_a_renamed_file_by_both_its_paths(cx: &mut TestAppContext) {
        let _dir = init(cx);
        let renamed = ChangedFile {
            status: FileStatus::Renamed,
            orig_path: Some("old.txt".into()),
            ..counted("new.txt")
        };
        set_changes(vec![], vec![renamed.clone()], false, cx);
        patches(
            &DiffSource::Unstaged,
            std::slice::from_ref(&renamed),
            "new",
            cx,
        );
        let opened = open(DiffSource::Unstaged, renamed, cx);
        opened.whole.respond(staged_whole());

        opened.click("stage-file", cx);
        assert_eq!(
            staging_paths(&[opened.whole.calls()[0].file.clone()]),
            ["new.txt", "old.txt"]
        );
    }

    #[gpui_kit::test]
    fn unstages_a_staged_file_then_opens_the_one_before_the_last(cx: &mut TestAppContext) {
        let _dir = init(cx);
        let staged = vec![counted("a.txt"), counted("b.txt")];
        set_changes(staged.clone(), vec![], false, cx);
        patches(&DiffSource::Staged, &staged, "staged", cx);
        let opened = open(DiffSource::Staged, counted("b.txt"), cx);
        opened.whole.respond(staged_whole());

        opened.click("stage-file", cx);
        assert_eq!(opened.whole.calls()[0].action, StageAction::Unstage);
        assert_eq!(
            opened.events(),
            [opened_with(DiffSource::Staged, counted("a.txt"))]
        );
    }

    #[gpui_kit::test]
    fn says_why_it_couldnt_be_staged(cx: &mut TestAppContext) {
        let _dir = init(cx);
        set_changes(vec![], vec![counted("a.txt")], false, cx);
        patches(&DiffSource::Unstaged, &[counted("a.txt")], "new", cx);
        let opened = open(DiffSource::Unstaged, counted("a.txt"), cx);
        opened
            .whole
            .respond(|_, _| Task::ready(Err(QueryError::msg("The index is locked."))));

        opened.click("stage-file", cx);
        assert_eq!(
            opened.staging_error(cx).as_deref(),
            Some("The index is locked.")
        );
        assert_eq!(opened.events(), []);
    }

    #[gpui_kit::test]
    fn opens_the_next_file_once_its_staged_without_waiting_for_the_lists(cx: &mut TestAppContext) {
        let _dir = init(cx);
        let unstaged = vec![counted("a.txt"), counted("b.txt")];
        set_changes(vec![], unstaged.clone(), false, cx);
        patches(&DiffSource::Unstaged, &unstaged, "new", cx);
        let opened = open(DiffSource::Unstaged, counted("a.txt"), cx);
        // The lists aren't reloaded: the status stays as it was.
        opened.whole.respond(staged_whole());

        opened.click("stage-file", cx);
        assert_eq!(
            opened.events(),
            [opened_with(DiffSource::Unstaged, counted("b.txt"))]
        );
    }

    #[gpui_kit::test]
    fn stages_nothing_else_while_its_staged_not_again_nor_its_lines(cx: &mut TestAppContext) {
        let _dir = init(cx);
        set_changes(vec![], vec![counted("a.txt")], false, cx);
        patches(&DiffSource::Unstaged, &[counted("a.txt")], "new", cx);
        let opened = open(DiffSource::Unstaged, counted("a.txt"), cx);
        let (done, finished) = oneshot::channel::<()>();
        let finished = Rc::new(RefCell::new(Some(finished)));
        opened.whole.respond(move |_, cx| {
            let finished = finished.borrow_mut().take();
            cx.background_spawn(async move {
                if let Some(finished) = finished {
                    let _ = finished.await;
                }
                Ok(())
            })
        });

        opened.click("stage-file", cx);
        assert!(!opened.is_enabled("stage-file", cx));
        assert!(opened.staging(cx).unwrap().busy);
        opened.click("stage-file", cx);
        assert_eq!(opened.whole.calls().len(), 1);
        done.send(()).unwrap();
        cx.run_until_parked();
        assert!(!opened.staging(cx).unwrap().busy);
    }

    #[gpui_kit::test]
    fn cant_be_staged_while_the_last_file_is_still_on_show_as_anothers_changes_load(
        cx: &mut TestAppContext,
    ) {
        let _dir = init(cx);
        let unstaged = vec![counted("a.txt"), counted("b.txt")];
        set_changes(vec![], unstaged, false, cx);
        patches(&DiffSource::Unstaged, &[counted("a.txt")], "new", cx);
        let _next = pending_patch(&DiffSource::Unstaged, &counted("b.txt"), cx);
        let opened = open(DiffSource::Unstaged, counted("a.txt"), cx);
        assert!(opened.shown(cx).is_some());

        opened.set_file(DiffSource::Unstaged, counted("b.txt"), cx);
        assert_eq!(opened.header(cx), "a.txt");
        assert!(!opened.is_enabled("stage-file", cx));
    }

    #[gpui_kit::test]
    fn isnt_offered_for_a_repository_inside_this_one(cx: &mut TestAppContext) {
        let _dir = init(cx);
        let nested = with_status(file("sub/", None, None), FileStatus::Untracked);
        set_changes(vec![], vec![nested.clone()], false, cx);
        let opened = open(DiffSource::Unstaged, nested, cx);

        assert_eq!(opened.body(cx), Body::Nested);
        assert!(!opened.has("stage-file", cx));
    }

    #[gpui_kit::test]
    fn isnt_offered_for_a_commits_file(cx: &mut TestAppContext) {
        let _dir = init(cx);
        commit_files(cx);
        let opened = open(commit(), counted("a.txt"), cx);

        assert!(opened.shown(cx).is_some());
        assert!(!opened.has("stage-file", cx));
    }
}
