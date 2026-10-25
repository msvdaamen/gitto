//! For the tests of resolving conflicts and editing files: a file's view, open, with what it reads
//! and writes made up, as the TS tests mocked the RPC.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use futures::FutureExt as _;
use futures::channel::oneshot;
use gitto_git::FileStatus;
use gitto_git::features::conflicts::schema::{Conflict, ConflictSide, ConflictText};
use gitto_git::features::diff::schema::ChangedFile;
use gitto_git::features::diff::working_tree::WorkingTreeFile;
use gitto_git::features::status::schema::{
    Head, StatusCounts, StatusReply, Uncommitted, WorkingTreeFiles,
};
use gitto_git::input::{FileInput, FullSha, NonEmpty, RepositoryId, Sha};
use gpui_kit::component::WindowExt as _;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Context, Entity, Global, Task, TestAppContext,
};

use super::conflicts::conflict_resolution::ResolveFileMutation;
use super::file_diff_view::{FileDiffEvent, FileDiffView};
use super::file_editing::{FileIo, Reload};
use crate::components::repository::test_support::{fake_git, repository_entity};
use crate::data::conflicts::FileToResolve;
use crate::data::file_diff::FilePatch;
use crate::data::git::{GitTask, RepositoryGit};
use crate::data::{DataError, Mutation, Repository};
use gitto_ui_logic::git::diff_source::{DiffSource, diff_file_key};

pub fn id() -> RepositoryId {
    RepositoryId::parse("01890a5d-ac96-774b-bcce-b302099a8057").unwrap()
}

/// Sets the app up, with the test repository's git made up (see [`DiffGit`]).
pub fn init(cx: &mut TestAppContext) -> tempfile::TempDir {
    let dir = crate::test_support::init(cx, |services| services);
    DiffGit::install(cx);
    dir
}

/// A patch the made-up git gives: always the same, or once, when the test sends it.
enum PatchAnswer {
    Now(Result<String, DataError>),
    Later(oneshot::Receiver<FilePatch>),
}

/// The test repository's git, made up: the status, patches and conflicted files it reads, which
/// the test sets as it sets what's loaded, so loading them again reads the same.
#[derive(Default)]
pub struct DiffGit {
    status: RefCell<Option<Uncommitted>>,
    /// By the file's key in its source (see `diff_file_key`).
    patches: RefCell<HashMap<String, PatchAnswer>>,
    conflicts: RefCell<HashMap<String, Conflict>>,
}

struct GlobalDiffGit(Rc<DiffGit>);

impl Global for GlobalDiffGit {}

impl DiffGit {
    /// Has the test repository ask it, from now on.
    pub fn install(cx: &mut TestAppContext) {
        let git = Rc::new(DiffGit::default());
        fake_git(&id(), Rc::clone(&git) as Rc<dyn RepositoryGit>, cx);
        cx.update(|cx| cx.set_global(GlobalDiffGit(git)));
    }

    pub fn get(cx: &App) -> Rc<DiffGit> {
        Rc::clone(&cx.global::<GlobalDiffGit>().0)
    }

    /// Reads the patch of the file `key` as `patch`, from now on.
    pub fn set_patch(&self, key: String, patch: Result<String, DataError>) {
        self.patches
            .borrow_mut()
            .insert(key, PatchAnswer::Now(patch));
    }

    /// Reads the patch of the file `key` once it's sent on the sender returned.
    pub fn later(&self, key: String) -> oneshot::Sender<FilePatch> {
        let (send, receive) = oneshot::channel();
        self.patches
            .borrow_mut()
            .insert(key, PatchAnswer::Later(receive));
        send
    }

    pub fn set_status(&self, status: Uncommitted) {
        *self.status.borrow_mut() = Some(status);
    }

    pub fn set_conflict(&self, path: &str, conflict: Conflict) {
        self.conflicts.borrow_mut().insert(path.into(), conflict);
    }

    fn patch(&self, source: DiffSource, file: &FileInput) -> GitTask<String> {
        let key = diff_file_key(&source, &file.path);
        let mut patches = self.patches.borrow_mut();
        match patches.remove(&key) {
            Some(PatchAnswer::Now(patch)) => {
                patches.insert(key, PatchAnswer::Now(patch.clone()));
                async move { patch }.boxed()
            }
            Some(PatchAnswer::Later(receive)) => async move {
                let patch = receive.await.map_err(|_| DataError::msg("Dropped."))?;
                Ok(patch.patch.to_string())
            }
            .boxed(),
            None => async move { Err(DataError::msg(format!("No patch of {key}."))) }.boxed(),
        }
    }
}

impl RepositoryGit for DiffGit {
    fn status(&self, _: Option<String>) -> GitTask<StatusReply> {
        let status = self.status.borrow().clone();
        async move {
            let status = status.ok_or_else(|| DataError::msg("No status in this test."))?;
            Ok(StatusReply::Status(Box::new(status)))
        }
        .boxed()
    }

    fn commit_file_patch(&self, sha: Sha, file: FileInput) -> GitTask<String> {
        let sha = sha.as_str().to_owned();
        self.patch(DiffSource::Commit { sha }, &file)
    }

    fn stash_file_patch(&self, sha: FullSha, file: FileInput) -> GitTask<String> {
        let sha = sha.as_str().to_owned();
        self.patch(DiffSource::Stash { sha }, &file)
    }

    fn unstaged_file_patch(&self, file: FileInput, _: bool) -> GitTask<String> {
        self.patch(DiffSource::Unstaged, &file)
    }

    fn staged_file_patch(&self, file: FileInput) -> GitTask<String> {
        self.patch(DiffSource::Staged, &file)
    }

    fn conflict(&self, path: NonEmpty) -> GitTask<Conflict> {
        let conflict = self.conflicts.borrow().get(path.as_str()).cloned();
        async move { conflict.ok_or_else(|| DataError::msg("Not conflicted.")) }.boxed()
    }
}

/// Changes what the test repository has loaded with `update`.
pub fn update_repository(
    cx: &mut TestAppContext,
    update: impl FnOnce(&mut Repository, &mut Context<Repository>),
) {
    cx.update(|cx| repository_entity(&id(), cx).update(cx, update));
}

pub fn file(path: &str, status: FileStatus) -> ChangedFile {
    ChangedFile {
        path: path.into(),
        status,
        orig_path: None,
        additions: Some(1),
        deletions: Some(1),
    }
}

pub fn conflicted(path: &str) -> ChangedFile {
    ChangedFile {
        path: path.into(),
        status: FileStatus::Conflicted,
        orig_path: None,
        additions: None,
        deletions: None,
    }
}

pub fn side(oid: &str) -> ConflictSide {
    ConflictSide {
        mode: "100644".into(),
        oid: oid.into(),
    }
}

/// A conflict both sides changed as text, read at `version`.
pub fn text_conflict(contents: &str, version: &str) -> Conflict {
    Conflict {
        base: Some(side("b1")),
        ours: Some(side("o1")),
        theirs: Some(side("t1")),
        text: Some(ConflictText {
            contents: contents.into(),
            version: version.into(),
        }),
        version: Some(version.into()),
        binary: false,
        unreadable: None,
    }
}

/// The uncommitted changes the status says there are.
pub fn set_changes(staged: Vec<ChangedFile>, unstaged: Vec<ChangedFile>, cx: &mut TestAppContext) {
    let conflicted = unstaged
        .iter()
        .filter(|file| file.status == FileStatus::Conflicted)
        .count() as u64;
    let status = Uncommitted {
        head: Head::Branch {
            name: "main".into(),
            sha: "a1".into(),
        },
        upstream: None,
        ahead: 0,
        behind: 0,
        counts: StatusCounts {
            files: (staged.len() + unstaged.len()) as u64,
            staged: staged.len() as u64,
            unstaged: unstaged.len() as u64,
            conflicted,
        },
        changes: WorkingTreeFiles {
            staged,
            unstaged,
            uncounted: false,
            marker_free: Vec::new(),
        },
        version: uuid::Uuid::new_v4().to_string(),
    };
    set_status(status, cx);
}

/// The status, as if it was read.
pub fn set_status(status: Uncommitted, cx: &mut TestAppContext) {
    cx.update(|cx| DiffGit::get(cx).set_status(status.clone()));
    update_repository(cx, |repository, cx| repository.set_status(status, cx));
}

/// The conflicted file at `path`, as if it was read.
pub fn set_conflict(path: &str, conflict: Conflict, cx: &mut TestAppContext) {
    cx.update(|cx| DiffGit::get(cx).set_conflict(path, conflict.clone()));
    update_repository(cx, |repository, cx| {
        repository.set_conflict(path, conflict, cx)
    });
}

/// Reads the conflicted file at `path` as `conflict` from now on, when it's loaded again.
pub fn reload_conflict_as(path: &str, conflict: Conflict, cx: &mut TestAppContext) {
    cx.update(|cx| DiffGit::get(cx).set_conflict(path, conflict));
}

/// Gives `file` of `source` the patch `patch`, as if it was loaded.
pub fn set_patch(source: &DiffSource, file: &ChangedFile, patch: &str, cx: &mut TestAppContext) {
    cx.update(|cx| set_patch_in(source, file, patch, cx));
}

/// [`set_patch`], in the app.
pub fn set_patch_in(source: &DiffSource, file: &ChangedFile, patch: &str, cx: &mut App) {
    let key = diff_file_key(source, &file.path);
    DiffGit::get(cx).set_patch(key.clone(), Ok(patch.to_owned()));
    let patch = FilePatch {
        key,
        file: file.clone(),
        patch: patch.into(),
    };
    repository_entity(&id(), cx)
        .update(cx, |repository, cx| repository.set_patch(source, patch, cx));
}

/// A patch of `path` with one line changed to `line`, as git would give it.
pub fn changed_line(path: &str, line: &str) -> String {
    format!(
        "diff --git a/{path} b/{path}\nindex 1111111..2222222 100644\n--- a/{path}\n+++ b/{path}\n@@ -1 +1 @@\n-before\n+{line}\n"
    )
}

/// A save asked of the working tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Save {
    pub path: String,
    pub contents: String,
    pub version: String,
    pub overwrite: bool,
}

pub fn save(path: &str, contents: &str, version: &str, overwrite: bool) -> Save {
    Save {
        path: path.into(),
        contents: contents.into(),
        version: version.into(),
        overwrite,
    }
}

type SaveReply = Box<dyn FnMut(&Save) -> Result<String, DataError>>;

/// The working tree a test makes up: the files it reads, and what saving them says.
#[derive(Clone)]
pub struct FakeTree {
    files: Rc<RefCell<HashMap<String, WorkingTreeFile>>>,
    saves: Rc<RefCell<Vec<Save>>>,
    reply: Rc<RefCell<SaveReply>>,
    reloads: Rc<RefCell<Vec<Reload>>>,
}

impl FakeTree {
    fn new() -> Self {
        FakeTree {
            files: Rc::default(),
            saves: Rc::default(),
            reply: Rc::new(RefCell::new(Box::new(|_| Ok("v2".into())))),
            reloads: Rc::default(),
        }
    }

    /// Reads `path` as `contents` at `version`.
    pub fn file(&self, path: &str, contents: &str, version: &str) {
        self.files.borrow_mut().insert(
            path.into(),
            WorkingTreeFile {
                contents: contents.into(),
                version: version.into(),
            },
        );
    }

    /// Answers saves with `reply` from now on.
    pub fn reply(&self, reply: impl FnMut(&Save) -> Result<String, DataError> + 'static) {
        *self.reply.borrow_mut() = Box::new(reply);
    }

    pub fn saves(&self) -> Vec<Save> {
        self.saves.borrow().clone()
    }

    /// What was asked to be loaded again, which isn't: it'd be read with git.
    pub fn reloads(&self) -> Vec<Reload> {
        self.reloads.borrow().clone()
    }

    fn io(&self) -> FileIo {
        let (files, saves, reply, reloads) = (
            Rc::clone(&self.files),
            Rc::clone(&self.saves),
            Rc::clone(&self.reply),
            Rc::clone(&self.reloads),
        );
        FileIo {
            read: Rc::new(move |_, path, _| {
                Task::ready(
                    files
                        .borrow()
                        .get(path)
                        .cloned()
                        .ok_or_else(|| DataError::msg("No such file.")),
                )
            }),
            save: Rc::new(move |_, path, contents, version, overwrite, _| {
                let asked = Save {
                    path: path.into(),
                    contents,
                    version,
                    overwrite,
                };
                saves.borrow_mut().push(asked.clone());
                Task::ready((reply.borrow_mut())(&asked))
            }),
            reload: Rc::new(move |_, reload, _| reloads.borrow_mut().push(reload)),
        }
    }
}

/// Resolving files whole, made up: it records what's asked, and does what the test says.
pub struct FakeResolve {
    mutation: ResolveFileMutation,
    calls: Rc<RefCell<Vec<FileToResolve>>>,
    failure: Rc<RefCell<Option<String>>>,
}

impl FakeResolve {
    fn new(cx: &mut TestAppContext) -> Self {
        let calls: Rc<RefCell<Vec<FileToResolve>>> = Rc::default();
        let failure: Rc<RefCell<Option<String>>> = Rc::default();
        let mutation = cx.update(|cx| {
            let (calls, failure) = (Rc::clone(&calls), Rc::clone(&failure));
            Mutation::new(
                move |input: FileToResolve, _: &mut App| {
                    calls.borrow_mut().push(input);
                    Task::ready(match failure.borrow_mut().take() {
                        Some(message) => Err(DataError::msg(message)),
                        None => Ok(()),
                    })
                },
                cx,
            )
        });
        FakeResolve {
            mutation,
            calls,
            failure,
        }
    }

    /// Fails the next time, for `message`.
    pub fn fail_once(&self, message: &str) {
        *self.failure.borrow_mut() = Some(message.into());
    }

    pub fn calls(&self) -> Vec<FileToResolve> {
        self.calls.borrow().clone()
    }
}

/// A file's view, open, with what it asked of the page.
pub struct Opened {
    pub window: AnyWindowHandle,
    pub view: Entity<FileDiffView>,
    events: Rc<RefCell<Vec<FileDiffEvent>>>,
    pub tree: FakeTree,
    pub resolve: FakeResolve,
}

impl Opened {
    pub fn events(&self) -> Vec<FileDiffEvent> {
        self.events.borrow().clone()
    }

    pub fn click(&self, element: impl Into<gpui_kit::ElementId>, cx: &mut TestAppContext) {
        let element = element.into();
        cx.update_window(self.window, |_, window, cx| {
            window.render_frame(cx);
            window.click(element, cx);
        })
        .unwrap();
        cx.run_until_parked();
    }

    pub fn press(&self, key: &str, cx: &mut TestAppContext) {
        cx.update_window(self.window, |_, window, cx| {
            window.render_frame(cx);
            window.press(key, cx)
        })
        .unwrap();
        cx.run_until_parked();
    }

    /// Types `text` where the focus is.
    pub fn input(&self, text: &str, cx: &mut TestAppContext) {
        cx.update_window(self.window, |_, window, cx| window.input(text, cx))
            .unwrap();
        cx.run_until_parked();
    }

    pub fn has(&self, element: impl Into<gpui_kit::ElementId>, cx: &mut TestAppContext) -> bool {
        let element = element.into();
        cx.update_window(self.window, |_, window, cx| {
            window.render_frame(cx);
            window.try_find(element).is_some()
        })
        .unwrap()
    }

    pub fn is_enabled(
        &self,
        element: impl Into<gpui_kit::ElementId>,
        cx: &mut TestAppContext,
    ) -> bool {
        let element = element.into();
        cx.update_window(self.window, |_, window, cx| {
            window.render_frame(cx);
            !window.find(element).disabled().unwrap_or(false)
        })
        .unwrap()
    }

    /// Whether a dialog is open over the window.
    pub fn has_dialog(&self, cx: &mut TestAppContext) -> bool {
        // Once one that's closing has gone.
        cx.executor()
            .advance_clock(std::time::Duration::from_secs(1));
        cx.run_until_parked();
        cx.update_window(self.window, |_, window, cx| window.has_active_dialog(cx))
            .unwrap()
    }

    pub fn read<R>(
        &self,
        cx: &mut TestAppContext,
        read: impl FnOnce(&FileDiffView, &App) -> R,
    ) -> R {
        cx.update(|cx| read(self.view.read(cx), cx))
    }
}

/// Opens `shown` of `source`, with a made-up working tree and resolving.
pub fn open(source: DiffSource, shown: ChangedFile, cx: &mut TestAppContext) -> Opened {
    let tree = FakeTree::new();
    let resolve = FakeResolve::new(cx);
    let io = tree.io();
    let mutation = resolve.mutation.clone();
    let (window, view) = crate::test_support::open(cx, |window, cx| {
        let mut view = FileDiffView::new(id(), source, shown, window, cx);
        view.set_io(io);
        view.set_resolve_mutation(mutation, cx);
        view
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
        tree,
        resolve,
    }
}
