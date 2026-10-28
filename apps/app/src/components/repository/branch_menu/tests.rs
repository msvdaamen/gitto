use std::cell::RefCell;
use std::rc::Rc;

use futures::FutureExt as _;
use gitto_git::features::operation::schema::Operation;
use gitto_git::features::refs::schema::Ref;
use gitto_git::features::status::schema::{Head, StatusReply, Uncommitted};
use gitto_git::input::{BranchRef, NonEmpty};
use gpui_kit::component::menu::ContextMenuExt as _;
use gpui_kit::prelude::*;
use gpui_kit::test::{TestSupportExt as _, TestWindowExt as _};
use gpui_kit::{AnyWindowHandle, IntoElement, Render, Subscription, TestAppContext, div};

use super::*;
use crate::components::repository::test_support::{
    click_label, conflicted, fake_git, has_dialog, repository_id, set_operation, set_refs,
    set_remotes, set_status, set_worktrees, setup, status, uncommitted, with_conflicts,
};
use gitto_app_data::data::DataError;
use gitto_app_data::data::git::{GitTask, RepositoryGit};

/// A sidebar's row for each branch, with its menu.
struct Host {
    provider: Entity<BranchMenuProvider>,
    branches: Vec<&'static str>,
    resolved: Rc<RefCell<Vec<ChangedFile>>>,
    _subscription: Subscription,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .children(self.branches.iter().map(|branch| {
                let full_name = SharedString::from(*branch);
                div()
                    .id(SharedString::from(format!("row-{branch}")))
                    .test_support()
                    .h_6()
                    .w_40()
                    .child(full_name.clone())
                    .context_menu(move |menu, window, cx| {
                        branch_menu(menu, full_name.clone(), window, cx)
                    })
            }))
    }
}

fn local(name: &str, upstream: Option<&str>, ahead: u64) -> Ref {
    Ref {
        name: name.into(),
        full_name: format!("refs/heads/{name}"),
        kind: RefKind::Local,
        sha: "abc".into(),
        current: false,
        upstream: upstream.map(Into::into),
        ahead,
        behind: 0,
    }
}

fn remote(name: &str) -> Ref {
    Ref {
        name: name.into(),
        full_name: format!("refs/remotes/{name}"),
        kind: RefKind::Remote,
        sha: "abc".into(),
        current: false,
        upstream: None,
        ahead: 0,
        behind: 0,
    }
}

fn worktree(path: &str, branch: &str, current: bool) -> Worktree {
    Worktree {
        path: path.into(),
        name: path.rsplit('/').next().unwrap().into(),
        head: Some("abc".into()),
        branch: Some(branch.into()),
        main: current,
        bare: false,
        current,
        locked: None,
        prunable: None,
    }
}

/// What the menus were asked to run.
#[derive(Default)]
struct Calls {
    merges: RefCell<Vec<Merge>>,
    deletes: RefCell<Vec<String>>,
    pulled_branches: RefCell<Vec<String>>,
    merge_outcome: RefCell<Option<MergeOutcome>>,
    /// The status as the merge leaves it, which the repository reloads.
    after_merge: RefCell<Option<Uncommitted>>,
    /// The status as it is, once it's been merged.
    now: RefCell<Option<Uncommitted>>,
}

/// The menus' repository's git, which records what it was asked to run.
impl RepositoryGit for Calls {
    fn merge_branch(&self, reference: BranchRef, into: NonEmpty) -> GitTask<MergeOutcome> {
        self.merges.borrow_mut().push(Merge { reference, into });
        *self.now.borrow_mut() = self.after_merge.borrow_mut().take();
        let outcome = self.merge_outcome.borrow_mut().take();
        async move { Ok(outcome.unwrap_or(MergeOutcome::Merged)) }.boxed()
    }

    fn delete_branch(&self, reference: BranchRef) -> GitTask<()> {
        self.deletes
            .borrow_mut()
            .push(reference.as_str().to_owned());
        async { Ok(()) }.boxed()
    }

    fn pull(&self, branch: Option<NonEmpty>) -> GitTask<()> {
        if let Some(branch) = branch {
            self.pulled_branches
                .borrow_mut()
                .push(branch.as_str().to_owned());
        }
        async { Ok(()) }.boxed()
    }

    // As the repository reloads after a merge.
    fn status(&self, _: Option<String>) -> GitTask<StatusReply> {
        let now = self.now.borrow().clone();
        async move {
            now.map(|status| StatusReply::Status(Box::new(status)))
                .ok_or(DataError::msg("Unchanged."))
        }
        .boxed()
    }
}

/// main checked out here, tracking origin/main; topic tracking origin/topic one commit
/// ahead; feature checked out in another worktree.
fn render(
    cx: &mut TestAppContext,
    calls: &Rc<Calls>,
) -> (
    tempfile::TempDir,
    RepositoryId,
    AnyWindowHandle,
    Entity<Host>,
) {
    let dir = setup(cx);
    let id = repository_id();
    set_status(&id, status(), cx);
    set_operation(&id, None, cx);
    set_refs(
        &id,
        vec![
            local("main", Some("origin/main"), 0),
            local("topic", Some("origin/topic"), 1),
            local("feature", None, 0),
            remote("origin/main"),
            remote("origin/topic"),
        ],
        cx,
    );
    set_worktrees(
        &id,
        vec![
            worktree("/work/gitto", "refs/heads/main", true),
            worktree("/work/gitto-feature", "refs/heads/feature", false),
        ],
        cx,
    );
    set_remotes(&id, &["origin"], cx);
    fake_git(&id, calls.clone(), cx);
    let shown = id.clone();
    let (window, host) = crate::test_support::open(cx, move |window, cx| {
        let create = cx.new(|cx| CreateBranchDialog::new(shown.clone(), window, cx));
        let add = cx.new(|cx| AddWorktreeDialog::new(shown.clone(), window, cx));
        let provider = cx.new(|cx| BranchMenuProvider::with(shown.clone(), create, add, cx));
        let resolved = Rc::new(RefCell::new(Vec::new()));
        let subscription = cx.subscribe(&provider, {
            let resolved = Rc::clone(&resolved);
            move |_, _, BranchMenuEvent::Resolve(file), _| resolved.borrow_mut().push(file.clone())
        });
        Host {
            provider,
            branches: vec![
                "refs/heads/main",
                "refs/heads/topic",
                "refs/heads/feature",
                "refs/remotes/origin/topic",
            ],
            resolved,
            _subscription: subscription,
        }
    });
    cx.run_until_parked();
    (dir, id, window, host)
}

/// The labels of the menu open, with whether each is disabled.
fn open_menu(window: AnyWindowHandle, row: &'static str, cx: &mut TestAppContext) -> Vec<String> {
    cx.update_window(window, |_, window, cx| window.right_click(row, cx))
        .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, _| {
        let menu = window.within("popup-menu");
        menu.find_all_by_role(gpui_kit::Role::MenuItem)
            .into_iter()
            .map(|item| item.label().unwrap_or_default().to_owned())
            .collect()
    })
    .unwrap()
}

fn choose(window: AnyWindowHandle, label: &str, cx: &mut TestAppContext) {
    cx.update_window(window, |_, window, cx| {
        let item = window.within("popup-menu").find_by_label(label);
        let center = item.bounds().center();
        use gpui_kit::test::TestEventExt as _;
        window.pointer_down(center, gpui_kit::MouseButton::Left, cx);
        window.pointer_up(center, gpui_kit::MouseButton::Left, cx);
    })
    .unwrap();
    cx.run_until_parked();
}

#[gpui_kit::test]
fn offers_merging_rebasing_a_worktree_and_deleting_for_another_branch(cx: &mut TestAppContext) {
    let calls = Rc::new(Calls::default());
    let (_dir, _, window, host) = render(cx, &calls);
    let items = open_menu(window, "row-refs/heads/topic", cx);
    assert_eq!(
        items,
        [
            "Create branch…",
            "Merge topic into main",
            "Rebase main onto topic",
            "Pull topic",
            "Add worktree…",
            "Delete topic…",
        ]
    );
    let provider = host.read_with(cx, |host, _| host.provider.clone());
    assert_eq!(
        provider.read_with(cx, |provider, _| provider.open_for().cloned()),
        Some("refs/heads/topic".into())
    );

    choose(window, "Pull topic", cx);
    assert_eq!(*calls.pulled_branches.borrow(), ["topic"]);
    assert_eq!(
        provider.read_with(cx, |provider, _| provider.open_for().cloned()),
        None
    );
}

#[gpui_kit::test]
fn doesnt_merge_the_checked_out_branch_into_itself_nor_delete_it(cx: &mut TestAppContext) {
    let calls = Rc::new(Calls::default());
    let (_dir, _, window, _) = render(cx, &calls);
    assert_eq!(
        open_menu(window, "row-refs/heads/main", cx),
        [
            "Create branch…",
            "Pull main",
            "Add worktree…",
            "Delete main…"
        ]
    );
    // Deleting it is disabled: it asks nothing.
    choose(window, "Delete main…", cx);
    assert!(!has_dialog(window, cx));
}

#[gpui_kit::test]
fn opens_the_worktree_a_branch_is_checked_out_in(cx: &mut TestAppContext) {
    let calls = Rc::new(Calls::default());
    let (_dir, _, window, host) = render(cx, &calls);
    let items = open_menu(window, "row-refs/heads/feature", cx);
    assert!(items.contains(&"Open worktree gitto-feature".to_owned()));
    assert!(!items.contains(&"Add worktree…".to_owned()));
    // Nor pulled from here, where it isn't checked out; nor deleted.
    assert!(!items.contains(&"Pull feature".to_owned()));
    let provider = host.read_with(cx, |host, _| host.provider.clone());
    assert!(provider.read_with(cx, |provider, cx| {
        provider.deletion("refs/heads/feature", cx).is_none()
    }));
}

#[gpui_kit::test]
fn doesnt_merge_during_an_operation_or_with_head_detached(cx: &mut TestAppContext) {
    let calls = Rc::new(Calls::default());
    let (_dir, id, window, _) = render(cx, &calls);
    set_operation(
        &id,
        Some(Operation::Merge {
            merging: "feature".into(),
            into: Some("main".into()),
        }),
        cx,
    );
    let items = open_menu(window, "row-refs/heads/topic", cx);
    assert!(items.contains(&"Merge topic into main".to_owned()));
    // Disabled: choosing it merges nothing.
    choose(window, "Merge topic into main", cx);
    assert!(calls.merges.borrow().is_empty());

    set_operation(&id, None, cx);
    set_status(&id, uncommitted(Head::Detached { sha: "abc".into() }), cx);
    let items = open_menu(window, "row-refs/heads/topic", cx);
    assert!(items.contains(&"Merge topic into detached HEAD".to_owned()));
    choose(window, "Merge topic into detached HEAD", cx);
    assert!(calls.merges.borrow().is_empty());
}

#[gpui_kit::test]
fn merges_and_opens_the_first_conflicted_file_when_it_stops_at_conflicts(cx: &mut TestAppContext) {
    let calls = Rc::new(Calls::default());
    let (_dir, _, window, host) = render(cx, &calls);
    *calls.merge_outcome.borrow_mut() = Some(MergeOutcome::Conflicts);
    *calls.after_merge.borrow_mut() =
        Some(with_conflicts(status(), &["a.txt", "b.txt"], &["a.txt"]));

    open_menu(window, "row-refs/heads/topic", cx);
    choose(window, "Merge topic into main", cx);
    assert_eq!(
        *calls.merges.borrow(),
        [Merge::parse("refs/heads/topic", "main").unwrap()]
    );
    let resolved = host.read_with(cx, |host, _| host.resolved.borrow().clone());
    // The one with markers left first.
    assert_eq!(resolved, [conflicted("b.txt")]);
}

#[gpui_kit::test]
fn deletes_a_remote_branch_with_the_local_ones_tracking_it_asking_first(cx: &mut TestAppContext) {
    let calls = Rc::new(Calls::default());
    let (_dir, _, window, host) = render(cx, &calls);
    let provider = host.read_with(cx, |host, _| host.provider.clone());
    assert_eq!(
        provider.read_with(cx, |provider, cx| provider
            .deletion("refs/remotes/origin/topic", cx)),
        Some(BranchDeletion::Remote {
            reference: "refs/remotes/origin/topic".into(),
            on_remote: true,
            tracking: vec![Unpushed {
                name: "topic".into(),
                ahead: 1,
            }],
        })
    );

    open_menu(window, "row-refs/remotes/origin/topic", cx);
    choose(window, "Delete origin/topic…", cx);
    assert!(has_dialog(window, cx));
    assert!(calls.deletes.borrow().is_empty());
    click_label(window, "Delete", cx);
    assert_eq!(*calls.deletes.borrow(), ["refs/remotes/origin/topic"]);
}

// `ref-list.test.tsx`'s, as it's the provider that asks.
#[gpui_kit::test]
fn forgets_the_branch_it_was_asking_about_deleting_in_another_repository(cx: &mut TestAppContext) {
    let calls = Rc::new(Calls::default());
    let (_dir, _, window, host) = render(cx, &calls);
    open_menu(window, "row-refs/heads/topic", cx);
    choose(window, "Delete topic…", cx);
    assert!(has_dialog(window, cx));

    let other = repository_id();
    set_status(&other, status(), cx);
    set_operation(&other, None, cx);
    set_refs(&other, vec![local("main", None, 0)], cx);
    set_worktrees(&other, vec![], cx);
    set_remotes(&other, &[], cx);
    let provider = host.read_with(cx, |host, _| host.provider.clone());
    cx.update_window(window, |_, window, cx| {
        provider.update(cx, |provider, cx| {
            provider.set_repository(other.clone(), window, cx)
        })
    })
    .unwrap();
    cx.run_until_parked();
    assert!(!has_dialog(window, cx));
    assert!(calls.deletes.borrow().is_empty());
}
