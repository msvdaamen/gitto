//! Editing an unstaged file in its diff, or a conflicted one in its conflicts' view. Ported from
//! `file-editing.tsx`.

use std::cell::RefCell;
use std::rc::Rc;

use futures::FutureExt as _;
use futures::channel::oneshot;
use futures::future::Shared;
use gitto_git::ContentVersion;
use gitto_git::features::diff::working_tree::WorkingTreeFile;
use gitto_git::input::RepositoryId;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::EditorState;
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, Context, Entity, Task, Window, div};

use super::autosave::{AUTOSAVE_DELAY, Autosave, ReadEdits, SaveEdits, SaveState};
use super::file_diff_view::FileDiffView;
use super::unsaved_changes_dialog::{UnsavedChoice, open_unsaved_changes_dialog};
use gitto_app_data::data::file_diff::{fetch_working_tree_file, save_working_tree_file};
use gitto_app_data::data::{DataError, Repositories};
use gitto_app_kit::theme::PaletteExt as _;
use gitto_app_kit::ui::{IconButton, IconName, Notice};

/// Whether a save goes over the file whatever it is now, or only as it was at the version
/// expected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overwrite {
    IfUnchanged,
    Always,
}

/// A file's text to save over it in the working tree: over the version `expected`, unless it's to
/// [`Overwrite::Always`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveRequest {
    pub contents: String,
    pub expected: ContentVersion,
    pub overwrite: Overwrite,
}

/// Editing the file on show: the version of it on disk the edits are of (see
/// `read_working_tree_file`), and the editor they're made in, which they're read from as they're
/// saved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditSession {
    pub version: ContentVersion,
    pub editor: Entity<EditorState>,
}

/// Reads a file in the working tree, with its version.
pub type ReadWorkingTreeFile =
    Rc<dyn Fn(&RepositoryId, &str, &mut App) -> Task<Result<WorkingTreeFile, DataError>>>;

/// Saves a file's text over it in the working tree, as the request says; resolves to its new
/// version.
pub type SaveWorkingTreeFile = Rc<
    dyn Fn(&RepositoryId, &str, SaveRequest, &mut App) -> Task<Result<ContentVersion, DataError>>,
>;

/// What of a repository is loaded again, as what it's of changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reload {
    /// The uncommitted changes: once a file's edits are saved, say.
    Uncommitted,
    /// The conflicted file at a path: once saving it failed, say, as it changed on disk.
    Conflict(String),
}

/// Loads again what of the repository it's given changed.
pub type ReloadData = Rc<dyn Fn(&RepositoryId, Reload, &mut App)>;

/// What the view reads and writes the working tree with: git, or a test's.
#[derive(Clone)]
pub struct FileIo {
    pub read: ReadWorkingTreeFile,
    pub save: SaveWorkingTreeFile,
    pub reload: ReloadData,
}

impl FileIo {
    /// With git (`fetchWorkingTreeFile`, `saveWorkingTreeFile`), and the repository's data.
    pub fn git() -> Self {
        FileIo {
            read: Rc::new(fetch_working_tree_file),
            save: Rc::new(|id, path, request, cx| {
                save_working_tree_file(
                    id,
                    path,
                    request.contents,
                    request.expected,
                    request.overwrite == Overwrite::Always,
                    cx,
                )
            }),
            // After a save, by when the repository may have been removed.
            reload: Rc::new(|id, reload, cx| {
                let Some(repository) = Repositories::get(id, cx) else {
                    return;
                };
                repository.update(cx, |repository, cx| match reload {
                    Reload::Uncommitted => drop(repository.reload_uncommitted(cx)),
                    Reload::Conflict(path) => drop(repository.reload_conflict(&path, cx)),
                })
            }),
        }
    }
}

/// The view's editing (`useFileEditing`): whether the file's edited, and saving it a moment after
/// each edit. Edit mode starts with the header's button and ends with it or Escape; leaving it,
/// the file or the repository saves what's left first, and asks what to do if that fails, e.g. as
/// the file changed on disk meanwhile.
pub struct FileEditing {
    editing: bool,
    /// The edits' version on disk, once editing started.
    session: Option<EditSession>,
    /// Why editing couldn't start.
    start_error: Option<String>,
    /// The file to edit again once it's on show as it is on disk, after its edits were dropped:
    /// only with that file's next patch, and not once another file is opened, or this one has no
    /// changes to show (see `cancel_reload`), so no file is edited without being asked.
    reloading: Option<String>,
    /// Saved to the file being edited when editing started, whichever is on show by then.
    target: Rc<RefCell<Option<(RepositoryId, String)>>>,
    io: Rc<RefCell<FileIo>>,
    autosave: Entity<Autosave>,
    /// Asking what to do with edits that couldn't be saved; answered with the user's choice.
    asking: Option<oneshot::Sender<UnsavedChoice>>,
    /// Leaving edit mode, once what's left is saved: asked again meanwhile, it's the same answer.
    leaving: Option<Shared<Task<bool>>>,
    /// Whether there's been something to save, to say how saving's going.
    edited: bool,
}

impl FileEditing {
    pub fn new(cx: &mut Context<FileDiffView>) -> Self {
        let io = Rc::new(RefCell::new(FileIo::git()));
        let target: Rc<RefCell<Option<(RepositoryId, String)>>> = Rc::default();
        let save: SaveEdits = {
            let (io, target) = (Rc::clone(&io), Rc::clone(&target));
            Rc::new(move |request, cx| {
                let Some((id, path)) = target.borrow().clone() else {
                    return Task::ready(Err(DataError::msg("Nothing is being edited.")));
                };
                let save = Rc::clone(&io.borrow().save);
                save(&id, &path, request, cx)
            })
        };
        let autosave = cx.new(|_| Autosave::new(save, AUTOSAVE_DELAY));
        cx.observe(&autosave, |_, _, cx| cx.notify()).detach();
        // Edits not saved yet as the app quits are sent as it does.
        cx.on_app_quit(|this, cx| {
            let sending = this
                .editing
                .autosave
                .update(cx, |autosave, cx| autosave.flush_now(cx));
            async move {
                if let Some(sending) = sending {
                    let _ = sending.await;
                }
            }
        })
        .detach();
        // Edits made just before the file closed by itself (its row went, say) are saved all
        // the same.
        cx.on_release(|this: &mut FileDiffView, cx| {
            let sending = this
                .editing
                .autosave
                .update(cx, |autosave, cx| autosave.flush_now(cx));
            if let Some(sending) = sending {
                sending.detach();
            }
        })
        .detach();
        FileEditing {
            editing: false,
            session: None,
            start_error: None,
            reloading: None,
            target,
            io,
            autosave,
            asking: None,
            leaving: None,
            edited: false,
        }
    }

    pub fn is_editing(&self) -> bool {
        self.editing
    }

    /// What the view reads and writes the working tree with.
    pub fn io(&self) -> FileIo {
        self.io.borrow().clone()
    }

    /// What the view reads and writes the working tree with, shared with what else does (see
    /// `ConflictResolutionView`): changed with [`FileDiffView::set_io`] for all of them.
    pub fn shared_io(&self) -> Rc<RefCell<FileIo>> {
        Rc::clone(&self.io)
    }

    pub fn autosave(&self) -> &Entity<Autosave> {
        &self.autosave
    }
}

impl FileDiffView {
    /// Reads and writes the working tree with `io` from now on: a test's.
    pub fn set_io(&mut self, io: FileIo) {
        *self.editing.io.borrow_mut() = io;
    }

    /// Starts editing the file on show.
    pub(super) fn start_editing(&mut self, cx: &mut Context<Self>) {
        let editing = &mut self.editing;
        editing.start_error = None;
        editing.reloading = None;
        *editing.target.borrow_mut() =
            Some((self.view.repository_id().clone(), self.view.file().path));
        editing.editing = true;
        self.view.set_editing(true, cx);
        cx.notify();
    }

    /// Stops editing, dropping the edits that aren't saved.
    pub(super) fn discard_edits(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(conflict) = self.resolution.read(cx).viewer().cloned() {
            conflict.update(cx, |conflict, cx| conflict.discard(window, cx));
        }
        self.close_patch_editor();
        self.editing
            .autosave
            .update(cx, |autosave, cx| autosave.stop(cx));
        self.stop_edit_mode(cx);
    }

    fn stop_edit_mode(&mut self, cx: &mut Context<Self>) {
        self.editing.editing = false;
        self.editing.session = None;
        self.view.set_editing(false, cx);
        // What was saved meanwhile is shown: the file's changes, held while it was edited, are
        // reloaded now, rather than whenever the watcher sees the saves (it doesn't watch the
        // working tree while the window is in the background).
        if std::mem::take(&mut self.editing.edited) {
            let reload = Rc::clone(&self.editing.io.borrow().reload);
            reload(self.view.repository_id(), Reload::Uncommitted, cx);
        }
        cx.notify();
    }

    /// Stops editing, once what's left is saved, or the user says what to do with it; resolves
    /// to whether editing stopped (or wasn't on), which leaving the file waits for. Asked again
    /// before that's known (Escape, then the back button, say), it's the same answer.
    pub fn leave(&mut self, cx: &mut Context<Self>) -> Shared<Task<bool>> {
        if let Some(leaving) = &self.editing.leaving {
            return leaving.clone();
        }
        let leaving = cx
            .spawn(async move |this, cx| {
                let left = save_and_stop(&this, cx).await;
                let _ = this.update(cx, |this, _| this.editing.leaving = None);
                left
            })
            .shared();
        self.editing.leaving = Some(leaving.clone());
        leaving
    }

    /// Drops the edits, and edits the file as it is on disk once that's on show.
    fn reload_edits(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let path = self
            .editing
            .target
            .borrow()
            .as_ref()
            .map(|(_, path)| path.clone());
        self.discard_edits(window, cx);
        self.editing.reloading = path;
    }

    /// Asks what to do with edits that couldn't be saved.
    fn ask(&mut self, cx: &mut Context<Self>) -> oneshot::Receiver<UnsavedChoice> {
        let (send, receive) = oneshot::channel();
        self.editing.asking = Some(send);
        let state = self.editing.autosave.read(cx).state().clone();
        let path = self
            .editing
            .target
            .borrow()
            .as_ref()
            .map(|(_, path)| path.clone())
            .unwrap_or_default();
        let window = self.window;
        let view = cx.entity().downgrade();
        // Not from inside this view's update: the window is opened over.
        cx.spawn(async move |_, cx| {
            let _ = window.update(cx, |_, window, cx| {
                open_unsaved_changes_dialog(
                    &path,
                    state.message().unwrap_or_default(),
                    matches!(state, SaveState::ChangedOnDisk(_)),
                    move |choice, _, cx| {
                        let _ = view.update(cx, |this, _| {
                            if let Some(asking) = this.editing.asking.take() {
                                let _ = asking.send(choice);
                            }
                        });
                    },
                    window,
                    cx,
                );
            });
        })
        .detach();
        receive
    }

    /// Escape (`CloseFileDiff`) leaves edit mode, rather than the file: only when there's a
    /// selection to collapse is it left to the editor. Whether it was for edit mode.
    pub(super) fn escape_editing(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.editing.editing || self.editing.asking.is_some() {
            return false;
        }
        let selection = self
            .patch_editor
            .as_ref()
            .is_some_and(|editor| editor.read(cx).has_selection(cx))
            || self
                .resolution
                .read(cx)
                .viewer()
                .is_some_and(|viewer| viewer.read(cx).has_selection(cx));
        if selection {
            cx.propagate();
        } else {
            drop(self.leave(cx));
        }
        true
    }

    /// Editing started (with the version of the file the edits are of), or stopped.
    pub(super) fn on_editing(&mut self, session: Option<EditSession>, cx: &mut Context<Self>) {
        self.editing.edited = false;
        match &session {
            Some(session) => {
                let version = session.version.clone();
                let editor = session.editor.clone();
                let edits: ReadEdits = Rc::new(move |cx| editor.read(cx).value().to_string());
                self.editing
                    .autosave
                    .update(cx, |autosave, cx| autosave.start(version, edits, cx));
            }
            None if self.editing.editing => {
                // Ended without being asked to (the viewer went): what's left is saved all the
                // same.
                self.stop_edit_mode(cx);
                let flushing = self
                    .editing
                    .autosave
                    .update(cx, |autosave, cx| autosave.flush(cx));
                let autosave = self.editing.autosave.downgrade();
                cx.spawn(async move |_, cx| {
                    flushing.await;
                    let _ = autosave.update(cx, |autosave, cx| autosave.stop(cx));
                })
                .detach();
            }
            None => {}
        }
        self.editing.session = session;
        cx.notify();
    }

    /// The file was edited.
    pub(super) fn on_edit(&mut self, cx: &mut Context<Self>) {
        self.editing.edited = true;
        self.editing
            .autosave
            .update(cx, |autosave, cx| autosave.change(cx));
    }

    /// Editing couldn't start, for `message`.
    pub(super) fn on_edit_failed(&mut self, message: String, cx: &mut Context<Self>) {
        self.close_patch_editor();
        self.stop_edit_mode(cx);
        self.editing.start_error = Some(message);
    }

    /// The patch went off the page, e.g. as the file has no changes left: no reload to finish.
    pub(super) fn cancel_reload(&mut self) {
        self.editing.reloading = None;
    }

    /// A patch was shown, or the conflicts: the file as it's on disk, after a reload.
    pub(super) fn on_shown_for_editing(&mut self, cx: &mut Context<Self>) {
        let reloaded = self.editing.reloading.take();
        if reloaded.is_some_and(|path| path == self.view.file().path) {
            self.start_editing(cx);
        }
    }

    /// How saving is going, next to the edit mode's button, once there's been something to save.
    fn save_status(&self, cx: &App) -> Option<&'static str> {
        if !self.editing.editing {
            return None;
        }
        match self.editing.autosave.read(cx).state() {
            SaveState::Pending | SaveState::Saving => Some("Saving…"),
            SaveState::Saved if self.editing.edited => Some("Saved"),
            _ => None,
        }
    }

    /// The edit mode's button, and how saving is going.
    pub(super) fn render_editing_controls(
        &self,
        editable: bool,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let palette = *cx.palette();
        let editing = self.editing.editing;
        let status = self.save_status(cx);
        let mut controls = Vec::new();
        if let Some(status) = status {
            controls.push(
                div()
                    .id("save-status")
                    .role(gpui_kit::Role::Status)
                    .aria_label(status)
                    .mr_1()
                    .text_size(gpui_kit::rems(0.733))
                    .text_color(palette.faint)
                    .child(status)
                    .into_any_element(),
            );
        }
        controls.push(
            IconButton::new(
                "edit-file",
                IconName::Pencil,
                if editing {
                    "Stop editing (Esc)"
                } else {
                    "Edit file"
                },
            )
            .active(editing)
            .disabled(!editing && !editable)
            .on_click(cx.listener(|this, _, _, cx| {
                if this.editing.editing {
                    drop(this.leave(cx));
                } else {
                    this.start_editing(cx);
                }
            }))
            .into_any_element(),
        );
        controls
    }

    /// Why the edits aren't saved, or editing didn't start, with what can be done about it.
    pub(super) fn render_editing_banner(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut banners = Vec::new();
        if self.editing.editing {
            match self.editing.autosave.read(cx).state().clone() {
                SaveState::ChangedOnDisk(message) => {
                    banners.push(
                        Notice::new(message)
                            .child(
                                Button::new("reload-edits")
                                    .ghost()
                                    .small()
                                    .label("Reload")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.reload_edits(window, cx)
                                    })),
                            )
                            .child(
                                Button::new("overwrite-edits")
                                    .ghost()
                                    .small()
                                    .label("Overwrite")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        let overwriting = this
                                            .editing
                                            .autosave
                                            .update(cx, |autosave, cx| autosave.overwrite(cx));
                                        overwriting.detach();
                                    })),
                            )
                            .into_any_element(),
                    )
                }
                SaveState::Failed(message) => banners.push(
                    Notice::new(message)
                        .child(
                            Button::new("retry-save")
                                .ghost()
                                .small()
                                .label("Try again")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    let flushing = this
                                        .editing
                                        .autosave
                                        .update(cx, |autosave, cx| autosave.flush(cx));
                                    flushing.detach();
                                })),
                        )
                        .into_any_element(),
                ),
                _ => {}
            }
        }
        if let Some(message) = &self.editing.start_error {
            banners
                .push(Notice::new(format!("Couldn't edit the file: {message}")).into_any_element());
        }
        banners
    }
}

/// Stops editing the view `this`, once what's left is saved, or the user says what to do with it
/// (`saveAndStop`); resolves to whether editing stopped.
async fn save_and_stop(
    this: &gpui_kit::WeakEntity<FileDiffView>,
    cx: &mut gpui_kit::AsyncApp,
) -> bool {
    let Ok(flushing) = this.update(cx, |this, cx| {
        this.editing.editing.then(|| {
            this.editing
                .autosave
                .update(cx, |autosave, cx| autosave.flush(cx))
        })
    }) else {
        return true;
    };
    let Some(flushing) = flushing else {
        return true;
    };
    if !flushing.await {
        let Ok(asking) = this.update(cx, |this, cx| this.ask(cx)) else {
            return false;
        };
        match asking.await.unwrap_or(UnsavedChoice::Cancel) {
            UnsavedChoice::Cancel => return false,
            UnsavedChoice::Discard => {
                let window = this.read_with(cx, |this, _| this.window).ok();
                let _ = window.map(|window| {
                    window.update(cx, |_, window, cx| {
                        let _ = this.update(cx, |this, cx| this.discard_edits(window, cx));
                    })
                });
                return true;
            }
            UnsavedChoice::Save => {
                let Ok(saving) = this.update(cx, |this, cx| {
                    this.editing.autosave.update(cx, |autosave, cx| {
                        if matches!(autosave.state(), SaveState::ChangedOnDisk(_)) {
                            autosave.overwrite(cx)
                        } else {
                            autosave.flush(cx)
                        }
                    })
                }) else {
                    return false;
                };
                if !saving.await {
                    return false;
                }
            }
        }
    }
    let _ = this.update(cx, |this, cx| {
        this.editing
            .autosave
            .update(cx, |autosave, cx| autosave.stop(cx));
        this.stop_edit_mode(cx);
    });
    true
}

#[cfg(test)]
mod tests {
    use gitto_git::DiskChange;
    use gitto_git::FileStatus;
    use gitto_git::{ApiError, Error};
    use gpui_kit::TestAppContext;

    use super::super::autosave::AUTOSAVE_DELAY;
    use super::super::file_diff_view::FileDiffEvent;
    use super::super::test_views::{
        DiffGit, Opened, changed_line, file, init, open, save, set_changes, set_patch,
        update_repository,
    };
    use super::*;
    use gitto_ui_logic::git::diff_source::{DiffFileKey, DiffSource};

    /// Saving fails as git's API says the file changed on disk.
    fn changed_on_disk() -> DataError {
        DataError::from(ApiError::unreported(Error::FileChangedOnDisk {
            path: "a.txt".into(),
            how: DiskChange::Changed,
        }))
    }

    /// `a.txt`'s unstaged changes, open: one line changed to `old`, which the file on disk has.
    fn open_unstaged(cx: &mut TestAppContext) -> Opened {
        let modified = file("a.txt", FileStatus::Modified);
        set_changes(vec![], vec![modified.clone()], cx);
        set_patch(
            &DiffSource::Unstaged,
            &modified,
            &changed_line("a.txt", "old"),
            cx,
        );
        let opened = open(DiffSource::Unstaged, modified, cx);
        opened.tree.file("a.txt", "old\n", "v0");
        opened
    }

    /// The patch on show.
    fn shown(opened: &Opened, cx: &mut TestAppContext) -> Option<String> {
        opened.read(cx, |view, cx| {
            let viewer = view.viewer()?;
            viewer.read(cx).shown_patch().map(str::to_owned)
        })
    }

    fn is_editing(opened: &Opened, cx: &mut TestAppContext) -> bool {
        opened.read(cx, |view, _| view.editing.is_editing())
    }

    /// Starts editing, once the file's read.
    fn start_editing(opened: &Opened, cx: &mut TestAppContext) {
        opened.click("edit-file", cx);
        assert!(opened.read(cx, |view, cx| {
            view.patch_editor
                .as_ref()
                .is_some_and(|editor| editor.read(cx).editor().is_some())
        }));
    }

    fn wait_for_autosave(cx: &mut TestAppContext) {
        cx.executor().advance_clock(AUTOSAVE_DELAY);
        cx.run_until_parked();
    }

    /// Leaves edit mode, as the page asks to; what it resolved to, once it has.
    fn leave(opened: &Opened, cx: &mut TestAppContext) -> Rc<RefCell<Option<bool>>> {
        let result = Rc::new(RefCell::new(None));
        let done = Rc::clone(&result);
        let leaving = opened.view.update(cx, |view, cx| view.leave(cx));
        cx.update(|cx| {
            cx.spawn(async move |_| *done.borrow_mut() = Some(leaving.await))
                .detach()
        });
        cx.run_until_parked();
        result
    }

    #[gpui_kit::test]
    fn saves_the_edits_once_the_typing_stops_over_the_version_theyre_of(cx: &mut TestAppContext) {
        let _dir = init(cx);
        let opened = open_unstaged(cx);
        assert_eq!(shown(&opened, cx), Some(changed_line("a.txt", "old")));
        start_editing(&opened, cx);

        opened.input("new ", cx);
        assert_eq!(
            opened.read(cx, |view, cx| view.save_status(cx)),
            Some("Saving…")
        );
        wait_for_autosave(cx);
        assert_eq!(
            opened.tree.saves(),
            [save("a.txt", "new old\n", "v0", false)]
        );
        assert_eq!(
            opened.read(cx, |view, cx| view.save_status(cx)),
            Some("Saved")
        );
    }

    #[gpui_kit::test]
    fn is_only_offered_for_a_working_tree_file_thats_there(cx: &mut TestAppContext) {
        let _dir = init(cx);
        let staged = file("a.txt", FileStatus::Modified);
        set_changes(
            vec![staged.clone()],
            vec![file("gone.txt", FileStatus::Deleted)],
            cx,
        );
        set_patch(
            &DiffSource::Staged,
            &staged,
            &changed_line("a.txt", "staged"),
            cx,
        );
        let opened = open(DiffSource::Staged, staged, cx);
        assert_eq!(shown(&opened, cx), Some(changed_line("a.txt", "staged")));
        assert!(!opened.has("edit-file", cx));
    }

    #[gpui_kit::test]
    fn holds_the_changes_reloaded_while_editing_and_shows_them_once_it_stops(
        cx: &mut TestAppContext,
    ) {
        let _dir = init(cx);
        let opened = open_unstaged(cx);
        start_editing(&opened, cx);

        set_patch(
            &DiffSource::Unstaged,
            &file("a.txt", FileStatus::Modified),
            &changed_line("a.txt", "saved"),
            cx,
        );
        cx.run_until_parked();
        assert_eq!(shown(&opened, cx), Some(changed_line("a.txt", "old")));

        opened.click("edit-file", cx);
        assert!(!is_editing(&opened, cx));
        assert_eq!(shown(&opened, cx), Some(changed_line("a.txt", "saved")));
    }

    #[gpui_kit::test]
    fn says_when_the_file_changed_on_disk_and_saves_over_it_when_asked(cx: &mut TestAppContext) {
        let _dir = init(cx);
        let opened = open_unstaged(cx);
        opened.tree.reply(|asked| {
            if asked.overwrite {
                Ok("v1".into())
            } else {
                Err(changed_on_disk())
            }
        });
        start_editing(&opened, cx);

        opened.input("mine ", cx);
        wait_for_autosave(cx);
        assert!(opened.has("overwrite-edits", cx));
        assert!(opened.read(cx, |view, cx| matches!(
            view.editing.autosave().read(cx).state(),
            SaveState::ChangedOnDisk(message)
                if message == "a.txt was changed on disk since you started editing it."
        )));

        opened.click("overwrite-edits", cx);
        assert_eq!(
            opened.tree.saves().last(),
            Some(&save("a.txt", "mine old\n", "v0", true))
        );
        assert!(!opened.has("overwrite-edits", cx));
    }

    #[gpui_kit::test]
    fn asks_before_leaving_edits_that_couldnt_be_saved_cancel_stays_discard_drops_them(
        cx: &mut TestAppContext,
    ) {
        let _dir = init(cx);
        let opened = open_unstaged(cx);
        opened.tree.reply(|_| Err(changed_on_disk()));
        start_editing(&opened, cx);
        opened.input("mine ", cx);
        wait_for_autosave(cx);
        assert!(opened.has("overwrite-edits", cx));

        let staying = leave(&opened, cx);
        assert!(opened.has_dialog(cx));
        opened.click("cancel", cx);
        assert_eq!(*staying.borrow(), Some(false));
        assert!(is_editing(&opened, cx));

        let leaving = leave(&opened, cx);
        // Discard, the first of the dialog's answers after Cancel.
        opened.click(("action", 0usize), cx);
        assert_eq!(*leaving.borrow(), Some(true));
        // The editor's gone, and what it told the view with it.
        assert!(opened.read(cx, |view, _| view.patch_editor.is_none()
            && view.patch_editor_events.is_none()));
        // Typed in meanwhile: the file's changes are read again.
        assert_eq!(opened.tree.reloads(), [Reload::Uncommitted]);
        assert!(!is_editing(&opened, cx));
    }

    #[gpui_kit::test]
    fn leaves_edit_mode_with_esc_and_closes_the_file_with_the_next(cx: &mut TestAppContext) {
        let _dir = init(cx);
        let opened = open_unstaged(cx);
        start_editing(&opened, cx);

        opened.press("escape", cx);
        assert!(!is_editing(&opened, cx));
        assert_eq!(opened.events(), []);
        opened.press("escape", cx);
        assert_eq!(opened.events(), [FileDiffEvent::Close]);
    }

    #[gpui_kit::test]
    fn asks_once_when_leaving_is_asked_for_twice_with_the_same_answer_for_both(
        cx: &mut TestAppContext,
    ) {
        let _dir = init(cx);
        let opened = open_unstaged(cx);
        opened.tree.reply(|_| Err(changed_on_disk()));
        start_editing(&opened, cx);
        opened.input("mine ", cx);
        wait_for_autosave(cx);

        // Esc, then the back button, say.
        let first = leave(&opened, cx);
        let second = leave(&opened, cx);
        opened.click("cancel", cx);
        assert_eq!(
            (*first.borrow(), *second.borrow()),
            (Some(false), Some(false))
        );
        assert!(!opened.has_dialog(cx));
    }

    #[gpui_kit::test]
    fn keeps_editing_when_the_changes_cant_be_reloaded_meanwhile(cx: &mut TestAppContext) {
        let _dir = init(cx);
        let opened = open_unstaged(cx);
        start_editing(&opened, cx);

        cx.update(|cx| {
            DiffGit::get(cx).set_patch(
                DiffFileKey::new(&DiffSource::Unstaged, "a.txt"),
                Err(DataError::msg("These changes are over 10 MB.")),
            )
        });
        update_repository(cx, |repository, cx| drop(repository.reload_uncommitted(cx)));
        cx.run_until_parked();

        assert!(opened.read(cx, |view, _| view.view.patch_error().is_none()));
        assert!(is_editing(&opened, cx));
        assert!(is_editing(&opened, cx));
    }
}
