//! The menu of what can be done with a branch, and what the branch menus share. Ported from
//! `components/repository/branch-menu.tsx`.
//!
//! TS's `BranchMenuProvider` was a context around the sidebar and the history; here it's an
//! entity, [`BranchMenuProvider`], which the repository page makes and keeps, and which is also
//! the window's current one (see [`BranchMenuProvider::current`]), so the sidebar and the history
//! find it as TS's `useContext` did. An element with a branch's menu (a row in the sidebar, a
//! label in the history) builds it with [`branch_menu`]:
//!
//! ```ignore
//! div()
//!     .id(("branch", full_name.clone()))
//!     .context_menu(move |menu, window, cx| branch_menu(menu, full_name.clone(), window, cx))
//! ```
//!
//! and shows which branch's menu is open with [`BranchMenuProvider::open_for`] (TS's
//! `useBranchMenuOpenFor`). A branch checked out in another worktree opens that worktree rather
//! than switching to it, which git refuses: [`BranchMenuProvider::open_worktree`] (TS's
//! `useOpenWorktreeFromBranch`).

use std::sync::Arc;

use gitto_git::features::branch::schema::{MergeOutcome, RebaseOutcome};
use gitto_git::features::diff::schema::ChangedFile;
use gitto_git::features::operation::schema::Operation;
use gitto_git::features::refs::schema::{Ref, RefKind};
use gitto_git::features::remote::remote_name::remote_of;
use gitto_git::features::status::schema::{Head, StatusSummary, Uncommitted};
use gitto_git::features::worktree::schema::Worktree;
use gitto_git::input::RepositoryId;
use gitto_ui::git::status::{branch_name, head_label, is_checked_out};
use gitto_ui::git::worktree::{can_open_worktree, tracking_branches, worktree_on};
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::{
    App, AppContext as _, Context, Entity, EventEmitter, Global, SharedString, Subscription, Task,
    WeakEntity, Window,
};

use super::BeforeChange;
use super::add_worktree_dialog::AddWorktreeDialog;
use super::create_branch_dialog::CreateBranchDialog;
use super::delete_branch_dialog::{BranchDeletion, Unpushed, open_delete_branch_dialog};
use super::row_menu::RowMenu;
use crate::components::ui::{IconName, context_menu_item};
use crate::data::branch::{
    BranchChange, Merge, Rebase, branches_changing, delete_branch_operation,
    merge_branch_operation, rebase_branch_operation,
};
use crate::data::progress::operation_in_progress_query;
use crate::data::refs::refs_query;
use crate::data::remote::{PullOperation, pull_branch_operation, pull_operation, remotes_query};
use crate::data::status::{first_conflicted_file, status_query};
use crate::data::worktree::{open_worktree_operation, worktrees_query};
use crate::data::{Opaque, Operations, Query, RepositoryOperation};

/// What the branch menus ask of the page.
#[derive(Debug, Clone, PartialEq)]
pub enum BranchMenuEvent {
    /// Opens a conflicted file's conflicts: a merge or rebase stopped at them.
    Resolve(ChangedFile),
}

/// The operations the branch menus run: the data layer's, or a test's.
#[derive(Clone)]
pub struct BranchActions {
    pub merge: BranchChange<Merge, MergeOutcome>,
    pub rebase: BranchChange<Rebase, RebaseOutcome>,
    pub delete: BranchChange<String>,
    pub pull: PullOperation,
    /// Pulls a branch that isn't checked out, by its short name.
    pub pull_branch: RepositoryOperation<String>,
    /// Opens a worktree, by its path.
    pub open_worktree: RepositoryOperation<String>,
}

impl BranchActions {
    /// The data layer's.
    pub fn new() -> Self {
        BranchActions {
            merge: merge_branch_operation(),
            rebase: rebase_branch_operation(),
            delete: delete_branch_operation(),
            pull: pull_operation(),
            pull_branch: pull_branch_operation(),
            open_worktree: open_worktree_operation(),
        }
    }
}

impl Default for BranchActions {
    fn default() -> Self {
        BranchActions::new()
    }
}

/// The window's current provider, for the menus in the sidebar and the history to find.
struct CurrentBranchMenu(WeakEntity<BranchMenuProvider>);

impl Global for CurrentBranchMenu {}

/// What the branch menus share (see the module's doc): the dialog naming a branch to create, so a
/// name kept to fix after a failure is there whichever menu it's opened from again, the dialog
/// placing a worktree, likewise, merging, rebasing, pulling, the dialog asking before deleting a
/// branch, and opening a worktree. A merge or rebase that stops at conflicts opens the first
/// conflicted file to resolve them ([`BranchMenuEvent::Resolve`]). Merging or rebasing waits for
/// [`set_before_change`](Self::set_before_change)'s guard, as either rewrites the working tree.
///
/// None of creating, merging, rebasing or deleting is possible while another change of the
/// branches (a switch, a merge, a deletion…) is running; the toolbar's current branch shows those
/// running, and why they failed.
pub struct BranchMenuProvider {
    repository_id: RepositoryId,
    status: Query<Opaque<Uncommitted>, StatusSummary>,
    operation: Query<Option<Operation>>,
    worktrees: Query<Arc<[Worktree]>>,
    refs: Query<Arc<[Ref]>>,
    remotes: Query<Vec<String>>,
    actions: BranchActions,
    create: Entity<CreateBranchDialog>,
    add_worktree: Entity<AddWorktreeDialog>,
    before_change: Option<BeforeChange>,
    menu: RowMenu,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<BranchMenuEvent> for BranchMenuProvider {}

impl BranchMenuProvider {
    /// The provider for the repository `repository_id`, which is the window's current one from
    /// now on (see [`current`](Self::current)).
    pub fn new(repository_id: RepositoryId, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let create = cx.new(|cx| CreateBranchDialog::new(repository_id.clone(), window, cx));
        let add_worktree = cx.new(|cx| AddWorktreeDialog::new(repository_id.clone(), window, cx));
        BranchMenuProvider::with(
            repository_id,
            BranchActions::new(),
            create,
            add_worktree,
            cx,
        )
    }

    /// With `actions`, and the dialogs: a test's.
    pub(crate) fn with(
        repository_id: RepositoryId,
        actions: BranchActions,
        create: Entity<CreateBranchDialog>,
        add_worktree: Entity<AddWorktreeDialog>,
        cx: &mut Context<Self>,
    ) -> Self {
        let current = cx.entity().downgrade();
        cx.set_global(CurrentBranchMenu(current));
        BranchMenuProvider {
            status: status_query(&repository_id, cx),
            operation: operation_in_progress_query(&repository_id, cx),
            worktrees: worktrees_query(&repository_id, cx),
            refs: refs_query(&repository_id, cx),
            remotes: remotes_query(&repository_id, cx),
            repository_id,
            actions,
            create,
            add_worktree,
            before_change: None,
            menu: RowMenu::default(),
            _subscriptions: vec![cx.observe_global::<Operations>(|_, cx| cx.notify())],
        }
    }

    /// The window's current provider, if there's one: the repository page's.
    pub fn current(cx: &App) -> Option<Entity<BranchMenuProvider>> {
        cx.try_global::<CurrentBranchMenu>()
            .and_then(|current| current.0.upgrade())
    }

    pub fn repository_id(&self) -> &RepositoryId {
        &self.repository_id
    }

    /// Of another repository: a menu open for one of this one's branches closes, and so do the
    /// dialogs, whose names are for the repository they were typed in.
    pub fn set_repository(
        &mut self,
        repository_id: RepositoryId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.repository_id == repository_id {
            return;
        }
        self.status = status_query(&repository_id, cx);
        self.operation = operation_in_progress_query(&repository_id, cx);
        self.worktrees = worktrees_query(&repository_id, cx);
        self.refs = refs_query(&repository_id, cx);
        self.remotes = remotes_query(&repository_id, cx);
        self.repository_id = repository_id.clone();
        self.menu.close(cx);
        self.create.update(cx, |dialog, cx| {
            dialog.set_repository(repository_id.clone(), window, cx)
        });
        self.add_worktree.update(cx, |dialog, cx| {
            dialog.set_repository(repository_id, window, cx)
        });
        cx.notify();
    }

    /// What merging or rebasing waits for, as they rewrite files in the working tree: the edits
    /// to a file on show saved.
    pub fn set_before_change(&mut self, guard: BeforeChange) {
        self.before_change = Some(guard);
    }

    /// The full ref name of the branch whose menu is open, if any, for its element to show which
    /// one it's for.
    pub fn open_for(&self) -> Option<&SharedString> {
        self.menu.open_for()
    }

    pub fn create_dialog(&self) -> &Entity<CreateBranchDialog> {
        &self.create
    }

    pub fn add_worktree_dialog(&self) -> &Entity<AddWorktreeDialog> {
        &self.add_worktree
    }

    /// Opens the worktree at `path` in Gitto, as a repository of its own: for a branch's element
    /// to open the worktree the branch is checked out in, rather than switching to it, which git
    /// refuses. The worktrees' section in the sidebar shows it running, and why it failed.
    pub fn open_worktree(&mut self, path: String, cx: &mut Context<Self>) {
        let _ = self
            .actions
            .open_worktree
            .run(&self.repository_id, path, cx);
    }

    /// Whether a worktree is being opened.
    pub fn is_opening_worktree(&self, cx: &App) -> bool {
        self.actions
            .open_worktree
            .state(&self.repository_id, cx)
            .running
    }

    fn head(&self) -> Option<Head> {
        self.status.data().map(|status| status.head)
    }

    /// The branch to merge into, or to rebase: the one checked out; `None` with HEAD detached.
    fn target_branch(&self) -> Option<String> {
        match self.head()? {
            Head::Detached { .. } => None,
            Head::Branch { name, .. } | Head::Unborn { name } => Some(name),
        }
    }

    /// Whether a branch can be merged into the checked-out one, or that one rebased onto it.
    fn may_integrate(&self, cx: &App) -> bool {
        let conflicted = self
            .status
            .data()
            .is_some_and(|status| status.counts.conflicted > 0);
        self.target_branch().is_some()
            && !conflicted
            && self.operation.data() == Some(None)
            && !branches_changing(&self.repository_id, cx)
    }

    /// Whether the local branch `reference` is checked out here.
    fn checked_out_here(&self, reference: &str) -> bool {
        self.head()
            .is_some_and(|head| is_checked_out(&head, reference))
    }

    /// Whether the local branch `reference` can be pulled from here: it tracks another, and is
    /// checked out here or in no worktree.
    fn is_pullable(&self, reference: &str) -> bool {
        let tracks = self.refs.data().is_some_and(|refs| {
            refs.iter()
                .any(|r| r.full_name == reference && r.upstream.is_some())
        });
        let worktrees = self.worktrees.data();
        tracks
            && (self.checked_out_here(reference)
                || worktree_on(worktrees.as_deref(), reference).is_none())
    }

    /// Whether the local branch `reference` is checked out, here or in another worktree.
    fn is_checked_out(&self, reference: &str) -> bool {
        let worktrees = self.worktrees.data();
        self.checked_out_here(reference) || worktree_on(worktrees.as_deref(), reference).is_some()
    }

    /// The other worktree the branch is checked out in, if one that can be opened is.
    fn worktree_of(&self, branch: &str) -> Option<Worktree> {
        let worktrees = self.worktrees.data();
        worktree_on(worktrees.as_deref(), branch)
            .filter(|worktree| can_open_worktree(worktree))
            .cloned()
    }

    /// What deleting the branch `reference` deletes with it, or keeps; `None` if git won't delete
    /// it, as it, or a local branch tracking it, is checked out, or until that's known: the
    /// branches, the status and the worktrees have loaded. Until then, the dialog couldn't say
    /// which local branches go with a remote one.
    pub fn deletion(&self, reference: &str) -> Option<BranchDeletion> {
        let all = self.refs.data()?;
        self.status.data()?;
        self.worktrees.data()?;
        if !reference.starts_with("refs/remotes/") {
            if self.is_checked_out(reference) {
                return None;
            }
            let branch = all.iter().find(|r| r.full_name == reference);
            // Its remote branch, if it tracks one that's there, rather than another local one.
            let upstream = branch.and_then(|branch| branch.upstream.clone());
            let remote = all.iter().any(|candidate| {
                candidate.kind == RefKind::Remote && Some(&candidate.name) == upstream.as_ref()
            });
            return Some(BranchDeletion::Local {
                reference: reference.to_owned(),
                upstream: if remote { upstream } else { None },
                ahead: if remote {
                    branch.map_or(0, |branch| branch.ahead)
                } else {
                    0
                },
            });
        }
        let tracking = tracking_branches(&all, reference);
        if tracking
            .iter()
            .any(|local| self.is_checked_out(&local.full_name))
        {
            return None;
        }
        // Taken to be there until the remotes have loaded, so it isn't played down.
        let on_remote = self
            .remotes
            .data()
            .is_none_or(|remotes| remote_of(&remotes, branch_name(reference)).is_some());
        Some(BranchDeletion::Remote {
            reference: reference.to_owned(),
            on_remote,
            tracking: tracking
                .iter()
                .map(|local| Unpushed {
                    name: local.name.clone(),
                    ahead: local.ahead,
                })
                .collect(),
        })
    }

    /// Begins creating a branch from `from`, by its full ref name, asking for the new one's name.
    pub fn create_from(&mut self, from: String, window: &mut Window, cx: &mut Context<Self>) {
        self.create
            .update(cx, |dialog, cx| dialog.open(from, window, cx));
    }

    /// Begins adding a worktree for `branch`, by its full ref name, asking where to put it.
    pub fn add_worktree_for(
        &mut self,
        branch: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.add_worktree
            .update(cx, |dialog, cx| dialog.open(branch, window, cx));
    }

    /// Begins deleting a branch, asking first. The toolbar shows it running, and why it failed.
    pub fn delete_branch(&mut self, deletion: BranchDeletion, window: &mut Window, cx: &mut App) {
        let delete = self.actions.delete.clone();
        let id = self.repository_id.clone();
        open_delete_branch_dialog(
            deletion,
            move |deletion, _, cx| {
                let _ = delete.run(&id, deletion.reference().to_owned(), cx);
            },
            window,
            cx,
        );
    }

    /// Pulls a local branch, by its full ref name: the checked-out one as the Pull button does,
    /// any other by fast-forwarding it to its upstream. The toolbar shows it running, and why it
    /// failed.
    pub fn pull(&mut self, reference: &str, cx: &mut Context<Self>) {
        if self.checked_out_here(reference) {
            let _ = self.actions.pull.pull(&self.repository_id, cx);
        } else {
            let _ = self.actions.pull_branch.run(
                &self.repository_id,
                branch_name(reference).to_owned(),
                cx,
            );
        }
    }

    /// Whether the branch `reference` is being pulled: the checked-out one, or another.
    fn is_pulling(&self, reference: &str, cx: &App) -> bool {
        if self.checked_out_here(reference) {
            self.actions.pull.state(&self.repository_id, cx).running
        } else {
            self.actions
                .pull_branch
                .state(&self.repository_id, cx)
                .running
        }
    }

    /// Merges the branch `reference`, by its full ref name, into the checked-out one, once the
    /// edits to a file on show are saved; if it stops at conflicts, opens the first conflicted
    /// file.
    pub fn merge(&mut self, reference: String, cx: &mut Context<Self>) {
        let Some(into) = self.target_branch() else {
            return;
        };
        let merge = self.actions.merge.clone();
        self.integrate(
            move |id, cx| merge.run(id, Merge { reference, into }, cx),
            |outcome: &MergeOutcome| *outcome == MergeOutcome::Conflicts,
            cx,
        );
    }

    /// Rebases the checked-out branch onto the branch `reference`, by its full ref name, as
    /// merging does.
    pub fn rebase(&mut self, reference: String, cx: &mut Context<Self>) {
        let Some(branch) = self.target_branch() else {
            return;
        };
        let rebase = self.actions.rebase.clone();
        self.integrate(
            move |id, cx| rebase.run(id, Rebase { reference, branch }, cx),
            |outcome: &RebaseOutcome| *outcome == RebaseOutcome::Conflicts,
            cx,
        );
    }

    /// Runs `run`, a merge or a rebase, once the edits to a file on show are saved; if it stops
    /// at conflicts (`conflicts` says), opens the first conflicted file.
    fn integrate<O: 'static>(
        &mut self,
        run: impl FnOnce(
            &RepositoryId,
            &mut App,
        ) -> Option<
            futures::channel::oneshot::Receiver<Result<O, crate::data::QueryError>>,
        > + 'static,
        conflicts: fn(&O) -> bool,
        cx: &mut Context<Self>,
    ) {
        let id = self.repository_id.clone();
        let saved = match &self.before_change {
            Some(before_change) => before_change(cx),
            None => Task::ready(true),
        };
        cx.spawn(async move |this, cx| {
            // Not once another repository is on show: the branch is this one's.
            if !saved.await {
                return;
            }
            let Ok(Some(done)) = this.update(cx, |this, cx| {
                if this.repository_id != id {
                    return None;
                }
                run(&id, cx)
            }) else {
                return;
            };
            let Ok(Ok(outcome)) = done.await else {
                return;
            };
            if !conflicts(&outcome) {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                if this.repository_id != id {
                    return;
                }
                // The status has reloaded by now (see `merge_branch_operation`).
                if let Some(file) = first_conflicted_file(&id, cx) {
                    cx.emit(BranchMenuEvent::Resolve(file));
                }
            });
        })
        .detach();
    }

    /// The menu for the branch `branch`, by its full ref name (see [`branch_menu`]).
    pub fn build_menu(
        this: &Entity<Self>,
        branch: SharedString,
        menu: PopupMenu,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        RowMenu::track(
            this.downgrade(),
            branch.clone(),
            |provider: &mut BranchMenuProvider| &mut provider.menu,
            cx,
        );
        let provider = this.read(cx);
        let busy = branches_changing(&provider.repository_id, cx);
        let short = branch_name(&branch).to_owned();
        let mut menu = menu.item(context_menu_item(
            IconName::GitBranchPlus,
            "Create branch…",
            busy,
            {
                let this = this.clone();
                let branch = branch.to_string();
                move |_, window, cx| {
                    let branch = branch.clone();
                    this.update(cx, |this, cx| this.create_from(branch, window, cx));
                }
            },
        ));
        if let Some(head) = provider.head()
            && !is_checked_out(&head, &branch)
        {
            let may_integrate = provider.may_integrate(cx);
            let head = head_label(&head).to_owned();
            menu = menu
                .item(context_menu_item(
                    IconName::GitMerge,
                    format!("Merge {short} into {head}"),
                    !may_integrate,
                    {
                        let this = this.clone();
                        let branch = branch.to_string();
                        move |_, _, cx| {
                            let branch = branch.clone();
                            this.update(cx, |this, cx| this.merge(branch, cx));
                        }
                    },
                ))
                .item(context_menu_item(
                    IconName::GitPullRequestArrow,
                    format!("Rebase {head} onto {short}"),
                    !may_integrate,
                    {
                        let this = this.clone();
                        let branch = branch.to_string();
                        move |_, _, cx| {
                            let branch = branch.clone();
                            this.update(cx, |this, cx| this.rebase(branch, cx));
                        }
                    },
                ));
        }
        if provider.is_pullable(&branch) {
            menu = menu.item(context_menu_item(
                IconName::Download,
                format!("Pull {short}"),
                provider.is_pulling(&branch, cx),
                {
                    let this = this.clone();
                    let branch = branch.clone();
                    move |_, _, cx| this.update(cx, |this, cx| this.pull(&branch, cx))
                },
            ));
        }
        menu = match provider.worktree_of(&branch) {
            Some(worktree) => menu.item(context_menu_item(
                IconName::FolderOpen,
                format!("Open worktree {}", worktree.name),
                provider.is_opening_worktree(cx),
                {
                    let this = this.clone();
                    let path = worktree.path.clone();
                    move |_, _, cx| {
                        let path = path.clone();
                        this.update(cx, |this, cx| this.open_worktree(path, cx));
                    }
                },
            )),
            None => menu.item(context_menu_item(
                IconName::FolderPlus,
                "Add worktree…",
                busy,
                {
                    let this = this.clone();
                    let branch = branch.to_string();
                    move |_, window, cx| {
                        let branch = branch.clone();
                        this.update(cx, |this, cx| this.add_worktree_for(branch, window, cx));
                    }
                },
            )),
        };
        let deletion = provider.deletion(&branch);
        menu.item(context_menu_item(
            IconName::Trash,
            format!("Delete {short}…"),
            busy || deletion.is_none(),
            {
                let this = this.clone();
                move |_, window, cx| {
                    if let Some(deletion) = deletion.clone() {
                        this.update(cx, |this, cx| this.delete_branch(deletion, window, cx));
                    }
                }
            },
        ))
    }
}

/// The menu of what can be done with the branch `branch`, by its full ref name, from the
/// window's current [`BranchMenuProvider`]: creating a branch from it, merging it into the
/// checked-out one, or rebasing the checked-out one onto it, neither of which is offered for that
/// one itself, pulling a local one that tracks another, unless it's checked out in another
/// worktree (where it's pulled), checking it out in a new worktree, or opening the one it's
/// checked out in, and deleting it, asking first. One that isn't checked out is pulled without
/// checking it out, by fast-forwarding it, which fails if it diverged from its upstream. Deleting
/// a local branch deletes it here only; deleting a remote one deletes it on its remote, and the
/// local branches tracking it with it.
///
/// None of creating, merging, rebasing or deleting is possible while another change of the
/// branches (a switch, a merge, a deletion…) is running. Merging and rebasing are also disabled
/// while HEAD is detached, or an operation (a merge, a rebase…) or conflicts are under way, and
/// deleting while the branch, or for a remote one a local branch tracking it, is checked out, here
/// or in another worktree, as git would refuse. (Nor will it delete one being rebased or bisected,
/// which only it tells.)
///
/// For an element's `.context_menu(…)`. Empty, so nothing opens, without a provider.
pub fn branch_menu(
    menu: PopupMenu,
    branch: impl Into<SharedString>,
    _: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    match BranchMenuProvider::current(cx) {
        Some(provider) => BranchMenuProvider::build_menu(&provider, branch.into(), menu, cx),
        None => menu,
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use gitto_git::features::status::schema::Head;
    use gpui_kit::component::menu::ContextMenuExt as _;
    use gpui_kit::prelude::*;
    use gpui_kit::test::{TestSupportExt as _, TestWindowExt as _};
    use gpui_kit::{AnyWindowHandle, IntoElement, Render, TestAppContext, div};

    use super::*;
    use crate::components::repository::test_support::{
        click_label, conflicted, has_dialog, repository_id, set_operation, set_refs, set_remotes,
        set_status, set_worktrees, setup, status, uncommitted, with_conflicts,
    };
    use crate::data::QueryError;
    use crate::data::remote::PullOperation;

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
    }

    fn actions(calls: &Rc<Calls>) -> BranchActions {
        let never = |name: &'static str| {
            RepositoryOperation::new(name, |_, _: String, cx: &mut App| {
                cx.background_spawn(async { Err(QueryError::msg("Not in this test.")) })
            })
        };
        BranchActions {
            merge: BranchChange::new(RepositoryOperation::new("merge", {
                let calls = Rc::clone(calls);
                move |id: RepositoryId, input: Merge, cx: &mut App| {
                    calls.merges.borrow_mut().push(input);
                    if let Some(status) = calls.after_merge.borrow_mut().take() {
                        crate::data::QueryClient::set_data(
                            &crate::data::keys::status(&id),
                            Opaque::new(status),
                            cx,
                        );
                    }
                    let outcome = calls.merge_outcome.borrow_mut().take();
                    cx.background_spawn(async move { Ok(outcome.unwrap_or(MergeOutcome::Merged)) })
                }
            })),
            rebase: BranchChange::new(RepositoryOperation::new(
                "rebase",
                |_, _: Rebase, cx: &mut App| {
                    cx.background_spawn(async { Ok(RebaseOutcome::Rebased) })
                },
            )),
            delete: BranchChange::new(RepositoryOperation::new("delete-branch", {
                let calls = Rc::clone(calls);
                move |_, reference: String, cx: &mut App| {
                    calls.deletes.borrow_mut().push(reference);
                    cx.background_spawn(async { Ok(()) })
                }
            })),
            pull: PullOperation::with_operation(RepositoryOperation::new(
                "pull",
                |_, (), cx: &mut App| cx.background_spawn(async { Ok(()) }),
            )),
            pull_branch: RepositoryOperation::new("pull-branch", {
                let calls = Rc::clone(calls);
                move |_, branch: String, cx: &mut App| {
                    calls.pulled_branches.borrow_mut().push(branch);
                    cx.background_spawn(async { Ok(()) })
                }
            }),
            open_worktree: never("open-worktree"),
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
        let actions = actions(calls);
        let shown = id.clone();
        let (window, host) = crate::test_support::open(cx, move |window, cx| {
            let create = cx.new(|cx| CreateBranchDialog::new(shown.clone(), window, cx));
            let add = cx.new(|cx| AddWorktreeDialog::new(shown.clone(), window, cx));
            let provider =
                cx.new(|cx| BranchMenuProvider::with(shown.clone(), actions, create, add, cx));
            let resolved = Rc::new(RefCell::new(Vec::new()));
            let subscription = cx.subscribe(&provider, {
                let resolved = Rc::clone(&resolved);
                move |_, _, BranchMenuEvent::Resolve(file), _| {
                    resolved.borrow_mut().push(file.clone())
                }
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
    fn open_menu(
        window: AnyWindowHandle,
        row: &'static str,
        cx: &mut TestAppContext,
    ) -> Vec<String> {
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
        assert!(provider.read_with(cx, |provider, _| {
            provider.deletion("refs/heads/feature").is_none()
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
    fn merges_and_opens_the_first_conflicted_file_when_it_stops_at_conflicts(
        cx: &mut TestAppContext,
    ) {
        let calls = Rc::new(Calls::default());
        let (_dir, _, window, host) = render(cx, &calls);
        *calls.merge_outcome.borrow_mut() = Some(MergeOutcome::Conflicts);
        *calls.after_merge.borrow_mut() =
            Some(with_conflicts(status(), &["a.txt", "b.txt"], &["a.txt"]));

        open_menu(window, "row-refs/heads/topic", cx);
        choose(window, "Merge topic into main", cx);
        assert_eq!(
            *calls.merges.borrow(),
            [Merge {
                reference: "refs/heads/topic".into(),
                into: "main".into(),
            }]
        );
        let resolved = host.read_with(cx, |host, _| host.resolved.borrow().clone());
        // The one with markers left first.
        assert_eq!(resolved, [conflicted("b.txt")]);
    }

    #[gpui_kit::test]
    fn deletes_a_remote_branch_with_the_local_ones_tracking_it_asking_first(
        cx: &mut TestAppContext,
    ) {
        let calls = Rc::new(Calls::default());
        let (_dir, _, window, host) = render(cx, &calls);
        let provider = host.read_with(cx, |host, _| host.provider.clone());
        assert_eq!(
            provider.read_with(cx, |provider, _| provider
                .deletion("refs/remotes/origin/topic")),
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
}
