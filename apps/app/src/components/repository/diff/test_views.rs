//! For the tests of resolving conflicts and editing files: a file's view, open, with what it reads
//! and writes made up, as the TS tests mocked the RPC.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use futures::FutureExt as _;
use gitto_git::features::conflicts::schema::{
    Conflict, ConflictFile, ConflictSide, ConflictSides, ConflictText,
};
use gitto_git::features::diff::schema::ChangedFile;
use gitto_git::features::diff::working_tree::WorkingTreeFile;
use gitto_git::features::status::schema::{
    Head, StatusCounts, StatusSummary, Uncommitted, WorkingTreeFiles,
};
use gitto_git::input::RepositoryId;
use gitto_git::{ContentVersion, FileMode, FileStatus};
use gpui_kit::component::WindowExt as _;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AnyWindowHandle, App, AppContext as _, Entity, Task, TestAppContext};

use super::conflict_resolution::ResolveFileMutation;
use super::file_diff_view::{FileDiffEvent, FileDiffView};
use super::file_editing::{FileIo, Overwrite};
use crate::data::conflicts::FileToResolve;
use crate::data::file_diff::{FilePatch, file_patch_key};
use crate::data::{Mutation, Opaque, QueryClient, QueryError, QueryKey, QueryOptions, keys};
use crate::git::diff_source::DiffSource;

pub fn id() -> RepositoryId {
    RepositoryId::parse("01890a5d-ac96-774b-bcce-b302099a8057").unwrap()
}

pub fn init(cx: &mut TestAppContext) -> tempfile::TempDir {
    crate::test_support::init(cx, |services| services)
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
        mode: FileMode::Regular,
        oid: oid.into(),
    }
}

/// A conflict both sides changed as text, read at `version`.
pub fn text_conflict(contents: &str, version: &str) -> Conflict {
    Conflict {
        sides: ConflictSides {
            base: Some(side("b1")),
            ours: Some(side("o1")),
            theirs: Some(side("t1")),
        },
        file: ConflictFile::Text(ConflictText {
            contents: contents.into(),
            version: version.into(),
        }),
    }
}

/// The uncommitted changes the status says there are.
pub fn set_changes(staged: Vec<ChangedFile>, unstaged: Vec<ChangedFile>, cx: &mut TestAppContext) {
    let conflicted = unstaged
        .iter()
        .filter(|file| file.status == FileStatus::Conflicted)
        .count() as u64;
    let status = Uncommitted {
        summary: StatusSummary {
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
        },
        changes: WorkingTreeFiles {
            staged,
            unstaged,
            uncounted: false,
            marker_free: Vec::new(),
        },
        version: uuid::Uuid::new_v4().to_string().into(),
    };
    cx.update(|cx| QueryClient::set_data(&keys::status(&id()), Opaque::new(status), cx));
}

/// The conflicted file at `path`, as if it was read.
pub fn set_conflict(path: &str, conflict: Conflict, cx: &mut TestAppContext) {
    cx.update(|cx| QueryClient::set_data(&keys::conflict(&id(), path), conflict, cx));
}

/// Reads the conflicted file at `path` as `conflict` from now on, when it's loaded again.
pub fn reload_conflict_as(path: &str, conflict: Conflict, cx: &mut TestAppContext) {
    let conflict = Arc::new(Mutex::new(conflict));
    let options = QueryOptions::new(keys::conflict(&id(), path), move |_, _| {
        let conflict = conflict.lock().unwrap().clone();
        async move { Ok(conflict) }.boxed()
    });
    cx.update(|cx| {
        let entry = QueryClient::entry::<Conflict>(options.key(), cx);
        entry.update(cx, |entry, _| entry.set_options(&options));
    });
}

/// Gives `file` of `source` the patch `patch`, as if it was loaded.
pub fn set_patch(source: &DiffSource, file: &ChangedFile, patch: &str, cx: &mut TestAppContext) {
    cx.update(|cx| {
        QueryClient::set_data(
            &file_patch_key(&id(), source, &file.path),
            FilePatch {
                key: source.file_key(&file.path),
                file: file.clone(),
                patch: patch.into(),
            },
            cx,
        )
    });
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

type SaveReply = Box<dyn FnMut(&Save) -> Result<String, QueryError>>;

/// The working tree a test makes up: the files it reads, and what saving them says.
#[derive(Clone)]
pub struct FakeTree {
    files: Rc<RefCell<HashMap<String, WorkingTreeFile>>>,
    saves: Rc<RefCell<Vec<Save>>>,
    reply: Rc<RefCell<SaveReply>>,
    reloads: Rc<RefCell<Vec<QueryKey>>>,
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
    pub fn reply(&self, reply: impl FnMut(&Save) -> Result<String, QueryError> + 'static) {
        *self.reply.borrow_mut() = Box::new(reply);
    }

    pub fn saves(&self) -> Vec<Save> {
        self.saves.borrow().clone()
    }

    /// What was asked to be loaded again, which isn't: it'd be read with git.
    pub fn reloads(&self) -> Vec<QueryKey> {
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
                        .ok_or_else(|| QueryError::msg("No such file.")),
                )
            }),
            save: Rc::new(move |_, path, request, _| {
                let asked = Save {
                    path: path.into(),
                    contents: request.contents,
                    version: request.expected.to_string(),
                    overwrite: request.overwrite == Overwrite::Always,
                };
                saves.borrow_mut().push(asked.clone());
                Task::ready((reply.borrow_mut())(&asked).map(ContentVersion::from))
            }),
            reload: Rc::new(move |key, _| reloads.borrow_mut().push(key)),
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
                        Some(message) => Err(QueryError::msg(message)),
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
