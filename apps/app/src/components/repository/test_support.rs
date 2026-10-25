//! For the repository area's tests: the app's globals, made-up repositories, and what their
//! queries hold. GPUI tests can't run git (see `crate::data`), so every query a view shows is
//! given its data first, and stays fresh: nothing loads it from git.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

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

use crate::data::repositories::RepositorySource;
use crate::data::{Opaque, QueryClient, QueryError, keys};

/// The repository list, answered at once: what the test puts in it.
#[derive(Default)]
pub struct Repositories {
    pub list: RefCell<Vec<Repository>>,
    /// The folders asked to be added, as opening a worktree does.
    pub added: RefCell<Vec<PathBuf>>,
}

impl RepositorySource for Repositories {
    fn list(&self) -> BoxFuture<'static, Result<Vec<Repository>, QueryError>> {
        let list = self.list.borrow().clone();
        async move { Ok(list) }.boxed()
    }

    fn add(&self, path: PathBuf) -> BoxFuture<'static, Result<Repository, QueryError>> {
        self.added.borrow_mut().push(path);
        async { Err(QueryError::msg("Not in this test.")) }.boxed()
    }

    fn remove(&self, _: String) -> BoxFuture<'static, Result<(), QueryError>> {
        async { Err(QueryError::msg("Not in this test.")) }.boxed()
    }

    fn reorder(&self, _: Vec<String>) -> BoxFuture<'static, Result<(), QueryError>> {
        async { Err(QueryError::msg("Not in this test.")) }.boxed()
    }
}

/// Sets up the app's globals, with an empty repository list; they keep their files in what's
/// returned, while it lives.
pub fn setup(cx: &mut TestAppContext) -> TempDir {
    setup_with(cx, Rc::new(Repositories::default()))
}

/// [`setup`], with the repository list `repositories`.
pub fn setup_with(cx: &mut TestAppContext, repositories: Rc<Repositories>) -> TempDir {
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

pub fn set_status(id: &RepositoryId, status: Uncommitted, cx: &mut TestAppContext) {
    cx.update(|cx| QueryClient::set_data(&keys::status(id), Opaque::new(status), cx));
}

pub fn set_stashes(id: &RepositoryId, stashes: Vec<Stash>, cx: &mut TestAppContext) {
    let stashes: Arc<[Stash]> = stashes.into();
    cx.update(|cx| QueryClient::set_data(&keys::stashes(id), stashes, cx));
}

pub fn set_push_target(id: &RepositoryId, target: PushTarget, cx: &mut TestAppContext) {
    cx.update(|cx| QueryClient::set_data(&keys::push_target(id), target, cx));
}

pub fn set_operation(id: &RepositoryId, operation: Option<Operation>, cx: &mut TestAppContext) {
    cx.update(|cx| QueryClient::set_data(&keys::operation(id), operation, cx));
}

pub fn set_refs(id: &RepositoryId, refs: Vec<Ref>, cx: &mut TestAppContext) {
    let refs: Arc<[Ref]> = refs.into();
    cx.update(|cx| QueryClient::set_data(&keys::refs(id), refs, cx));
}

pub fn set_worktrees(id: &RepositoryId, worktrees: Vec<Worktree>, cx: &mut TestAppContext) {
    let worktrees: Arc<[Worktree]> = worktrees.into();
    cx.update(|cx| QueryClient::set_data(&keys::worktrees(id), worktrees, cx));
}

pub fn set_remotes(id: &RepositoryId, remotes: &[&str], cx: &mut TestAppContext) {
    let remotes: Vec<String> = remotes.iter().map(|remote| (*remote).to_owned()).collect();
    cx.update(|cx| QueryClient::set_data(&keys::remotes(id), remotes, cx));
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
