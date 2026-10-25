//! For the repository area's tests: the app's globals, made-up repositories, and what their
//! entities hold. GPUI tests can't run git (see `crate::data`), so all a view shows of a
//! repository is given to its entity first, and stays fresh: its git, `NoGit`, fails, and
//! nothing reloads it.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use futures::FutureExt as _;
use futures::future::BoxFuture;

use gitto_git::features::diff::schema::ChangedFile;
use gitto_git::features::operation::schema::Operation;
use gitto_git::features::refs::schema::Ref;
use gitto_git::features::remote::schema::PushTarget;
use gitto_git::features::stash::schema::Stash;
use gitto_git::features::status::schema::{Head, StatusCounts, Uncommitted, WorkingTreeFiles};
use gitto_git::features::worktree::schema::Worktree;
use gitto_git::input::RepositoryId;
use gitto_repository::Repository;
use gpui_kit::component::WindowExt as _;
use gpui_kit::test::{TestEventExt as _, TestQueryExt as _};
use gpui_kit::{AnyWindowHandle, AppContext as _, MouseButton, Pixels, Point, TestAppContext};
use tempfile::TempDir;

use crate::data::DataError;
use crate::data::diff::{ChangedFiles, FilesOf};
use crate::data::file_diff::FilePatch;
use crate::data::git::{NoGit, RepositoryGit};
use crate::data::history::Log;
use crate::data::repositories::RepositorySource;
use gitto_ui_logic::git::diff_source::DiffSource;

/// The repository list, answered at once: what the test puts in it.
#[derive(Default)]
pub struct Repositories {
    pub list: RefCell<Vec<Repository>>,
    /// The folders asked to be added, as opening a worktree does.
    pub added: RefCell<Vec<PathBuf>>,
}

impl RepositorySource for Repositories {
    fn list(&self) -> BoxFuture<'static, Result<Vec<Repository>, DataError>> {
        let list = self.list.borrow().clone();
        async move { Ok(list) }.boxed()
    }

    fn add(&self, path: PathBuf) -> BoxFuture<'static, Result<Repository, DataError>> {
        self.added.borrow_mut().push(path);
        async { Err(DataError::msg("Not in this test.")) }.boxed()
    }

    fn remove(&self, _: String) -> BoxFuture<'static, Result<(), DataError>> {
        async { Err(DataError::msg("Not in this test.")) }.boxed()
    }

    fn reorder(&self, _: Vec<String>) -> BoxFuture<'static, Result<(), DataError>> {
        async { Err(DataError::msg("Not in this test.")) }.boxed()
    }
}

/// Sets up the app's globals, with an empty repository list; they keep their files in what's
/// returned, while it lives.
pub fn setup(cx: &mut TestAppContext) -> TempDir {
    setup_with(cx, Rc::new(Repositories::default()))
}

/// [`setup`], with the repository list `repositories`.
pub fn setup_with(cx: &mut TestAppContext, repositories: Rc<dyn RepositorySource>) -> TempDir {
    crate::test_support::init(cx, |services| services.with_repository_source(repositories))
}

/// A new repository's id.
pub fn repository_id() -> RepositoryId {
    RepositoryId::parse(uuid::Uuid::now_v7().to_string()).unwrap()
}

/// The uncommitted changes with HEAD at `head`: none, nor an upstream.
pub fn uncommitted(head: Head) -> Uncommitted {
    Uncommitted {
        head,
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
            staged: vec![],
            unstaged: vec![],
            uncounted: false,
            marker_free: vec![],
        },
        version: "v1".into(),
    }
}

/// On main, tracking origin/main, two commits behind it, without changes.
pub fn status() -> Uncommitted {
    Uncommitted {
        upstream: Some("origin/main".into()),
        behind: 2,
        ..uncommitted(Head::Branch {
            name: "main".into(),
            sha: "abc".into(),
        })
    }
}

/// `status`, with `files` changed: one staged, the rest unstaged.
pub fn changed(mut status: Uncommitted, files: u64) -> Uncommitted {
    status.counts = StatusCounts {
        files,
        staged: files.min(1),
        unstaged: files.saturating_sub(1),
        conflicted: 0,
    };
    status
}

/// A conflicted file, as the status lists it.
pub fn conflicted(path: &str) -> ChangedFile {
    serde_json::from_value(serde_json::json!({
        "path": path,
        "status": "conflicted",
        "origPath": null,
        "additions": null,
        "deletions": null,
    }))
    .unwrap()
}

/// `status`, with `paths` conflicted, of which `marker_free` have no markers left.
pub fn with_conflicts(
    mut status: Uncommitted,
    paths: &[&str],
    marker_free: &[&str],
) -> Uncommitted {
    let count = paths.len() as u64;
    status.counts = StatusCounts {
        files: count,
        staged: 0,
        unstaged: 0,
        conflicted: count,
    };
    status.changes.unstaged = paths.iter().map(|path| conflicted(path)).collect();
    status.changes.marker_free = marker_free.iter().map(|path| (*path).to_owned()).collect();
    status.version = uuid::Uuid::now_v7().to_string();
    status
}

/// The repository `id`'s entity, asking `NoGit` if it wasn't opened: its git fails, so only what
/// the test gives it is shown.
pub fn repository_entity(
    id: &RepositoryId,
    cx: &mut gpui_kit::App,
) -> gpui_kit::Entity<crate::data::Repository> {
    use crate::data::Repositories as Open;
    Open::get(id, cx).unwrap_or_else(|| Open::insert(id, Rc::new(NoGit), cx))
}

/// Has the repository `id` ask `git`, which makes up what the test's view asks of it.
pub fn fake_git(id: &RepositoryId, git: Rc<dyn RepositoryGit>, cx: &mut TestAppContext) {
    cx.update(|cx| {
        repository_entity(id, cx).update(cx, |repository, _| repository.set_git(git));
    });
}

/// Changes the repository `id`'s entity with `update`.
fn update(
    id: &RepositoryId,
    cx: &mut TestAppContext,
    update: impl FnOnce(&mut crate::data::Repository, &mut gpui_kit::Context<crate::data::Repository>),
) {
    cx.update(|cx| repository_entity(id, cx).update(cx, update));
}

pub fn set_status(id: &RepositoryId, status: Uncommitted, cx: &mut TestAppContext) {
    update(id, cx, |repository, cx| repository.set_status(status, cx));
}

pub fn set_stashes(id: &RepositoryId, stashes: Vec<Stash>, cx: &mut TestAppContext) {
    update(id, cx, |repository, cx| repository.set_stashes(stashes, cx));
}

pub fn set_push_target(id: &RepositoryId, target: PushTarget, cx: &mut TestAppContext) {
    update(id, cx, |repository, cx| {
        repository.set_push_target(target, cx)
    });
}

pub fn set_operation(id: &RepositoryId, operation: Option<Operation>, cx: &mut TestAppContext) {
    update(id, cx, |repository, cx| {
        repository.set_operation(operation, cx)
    });
}

pub fn set_refs(id: &RepositoryId, refs: Vec<Ref>, cx: &mut TestAppContext) {
    update(id, cx, |repository, cx| repository.set_refs(refs, cx));
}

pub fn set_worktrees(id: &RepositoryId, worktrees: Vec<Worktree>, cx: &mut TestAppContext) {
    update(id, cx, |repository, cx| {
        repository.set_worktrees(worktrees, cx)
    });
}

pub fn set_remotes(id: &RepositoryId, remotes: &[&str], cx: &mut TestAppContext) {
    let remotes: Vec<String> = remotes.iter().map(|remote| (*remote).to_owned()).collect();
    update(id, cx, |repository, cx| repository.set_remotes(remotes, cx));
}

pub fn set_log(id: &RepositoryId, log: Log, cx: &mut TestAppContext) {
    update(id, cx, |repository, cx| repository.set_log(log, cx));
}

/// The files the commit or stash `files.sha` changed, as `of`'s.
pub fn set_files(id: &RepositoryId, of: FilesOf, files: ChangedFiles, cx: &mut TestAppContext) {
    update(id, cx, |repository, cx| repository.set_files(of, files, cx));
}

/// The patch of a file in `source`, as if it had loaded.
pub fn set_patch(
    id: &RepositoryId,
    source: &DiffSource,
    patch: FilePatch,
    cx: &mut TestAppContext,
) {
    update(id, cx, |repository, cx| {
        repository.set_patch(source, patch, cx)
    });
}

/// The repository `id` at `path`, named after its folder.
pub fn repository(id: &RepositoryId, path: &str) -> Repository {
    Repository {
        id: id.to_string(),
        name: path.rsplit('/').next().unwrap_or(path).to_owned(),
        path: path.to_owned(),
    }
}

/// A stash made on `abc`.
pub fn stash(sha: char, message: &str) -> Stash {
    Stash {
        sha: sha.to_string().repeat(40),
        base: "abc".into(),
        message: message.into(),
        created_at: 1,
    }
}

/// Whether a dialog is open in `window`.
pub fn has_dialog(window: AnyWindowHandle, cx: &mut TestAppContext) -> bool {
    cx.update_window(window, |_, window, cx| window.has_active_dialog(cx))
        .unwrap()
}

/// Clicks what's labelled `label` in `window`, e.g. a dialog's button.
pub fn click_label(window: AnyWindowHandle, label: &str, cx: &mut TestAppContext) {
    let center = cx
        .update_window(window, |_, window, _| {
            window.find_by_label(label).bounds().center()
        })
        .unwrap();
    click_at(window, center, cx);
}

/// Clicks the last of what's labelled `label` in `window`: a dialog's button, over one labelled
/// the same under it.
pub fn click_last_label(window: AnyWindowHandle, label: &str, cx: &mut TestAppContext) {
    let center = cx
        .update_window(window, |_, window, _| {
            let found = window.find_all_by_label(label);
            found.last().unwrap().bounds().center()
        })
        .unwrap();
    click_at(window, center, cx);
}

fn click_at(window: AnyWindowHandle, at: Point<Pixels>, cx: &mut TestAppContext) {
    cx.update_window(window, |_, window, cx| {
        window.pointer_down(at, MouseButton::Left, cx);
        window.pointer_up(at, MouseButton::Left, cx);
    })
    .unwrap();
    cx.run_until_parked();
}
