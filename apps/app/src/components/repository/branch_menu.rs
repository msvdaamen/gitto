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

use std::cell::Cell;
use std::rc::Rc;

use gitto_git::features::branch::schema::{MergeOutcome, RebaseOutcome};
use gitto_git::features::diff::schema::ChangedFile;
use gitto_git::features::refs::schema::RefKind;
use gitto_git::features::remote::remote_name::remote_of;
use gitto_git::features::status::schema::Head;
use gitto_git::features::worktree::schema::Worktree;
use gitto_git::input::RepositoryId;
use gitto_ui::git::status::{branch_name, head_label, is_checked_out};
use gitto_ui::git::worktree::{can_open_worktree, tracking_branches, worktree_on};
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::{
    App, AppContext as _, Context, Entity, EventEmitter, Global, SharedString, Task, WeakEntity,
    Window,
};

use super::BeforeChange;
use super::add_worktree_dialog::AddWorktreeDialog;
use super::create_branch_dialog::CreateBranchDialog;
use super::delete_branch_dialog::{BranchDeletion, Unpushed, open_delete_branch_dialog};
use super::row_menu::RowMenu;
use crate::components::ui::{IconName, context_menu_item};
use crate::data::branch::{Merge, Rebase};
use crate::data::changes::{OPEN_WORKTREE, PULL, PULL_BRANCH};
use crate::data::status::{first_to_resolve, select_conflicted};
use crate::data::{Done, Part, Repository, Shown};

/// What the branch menus ask of the page.
#[derive(Debug, Clone, PartialEq)]
pub enum BranchMenuEvent {
    /// Opens a conflicted file's conflicts: a merge or rebase stopped at them.
    Resolve(ChangedFile),
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
    repository: Shown,
    create: Entity<CreateBranchDialog>,
    add_worktree: Entity<AddWorktreeDialog>,
    before_change: Option<BeforeChange>,
    menu: RowMenu,
    /// Whether the dialog asking before deleting a branch is open: it's of this repository.
    asking_delete: Rc<Cell<bool>>,
}

impl EventEmitter<BranchMenuEvent> for BranchMenuProvider {}

impl BranchMenuProvider {
    /// The provider for the repository `repository_id`, which is the window's current one from
    /// now on (see [`current`](Self::current)).
    pub fn new(repository_id: RepositoryId, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let create = cx.new(|cx| CreateBranchDialog::new(repository_id.clone(), window, cx));
        let add_worktree = cx.new(|cx| AddWorktreeDialog::new(repository_id.clone(), window, cx));
        BranchMenuProvider::with(repository_id, create, add_worktree, cx)
    }

    /// With the dialogs: a test's.
    pub(crate) fn with(
        repository_id: RepositoryId,
        create: Entity<CreateBranchDialog>,
        add_worktree: Entity<AddWorktreeDialog>,
        cx: &mut Context<Self>,
    ) -> Self {
        let current = cx.entity().downgrade();
        cx.set_global(CurrentBranchMenu(current));
        BranchMenuProvider {
            repository: Shown::new(&repository_id, PARTS, cx),
            create,
            add_worktree,
            before_change: None,
            menu: RowMenu::default(),
            asking_delete: Rc::default(),
        }
    }

    /// The window's current provider, if there's one: the repository page's.
    pub fn current(cx: &App) -> Option<Entity<BranchMenuProvider>> {
        cx.try_global::<CurrentBranchMenu>()
            .and_then(|current| current.0.upgrade())
    }

    pub fn repository_id<'a>(&self, cx: &'a App) -> &'a RepositoryId {
        self.repository.id(cx)
    }

    /// Of another repository: a menu open for one of this one's branches closes, and so do the
    /// dialogs, whose names are for the repository they were typed in.
    pub fn set_repository(
        &mut self,
        repository_id: RepositoryId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.repository.set(&repository_id, cx) {
            return;
        }
        self.menu.close(cx);
        if self.asking_delete.replace(false) {
            use gpui_kit::component::WindowExt as _;
            window.close_dialog(cx);
        }
        self.create.update(cx, |dialog, cx| {
            dialog.set_repository(repository_id.clone(), window, cx)
        });
        self.add_worktree.update(cx, |dialog, cx| {
            dialog.set_repository(repository_id, window, cx)
        });
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
        self.repository.update(cx, |repository, cx| {
            drop(repository.open_worktree(path, cx))
        });
    }

    /// Whether a worktree is being opened.
    pub fn is_opening_worktree(&self, cx: &App) -> bool {
        self.repository
            .read(cx)
            .operation_state(OPEN_WORKTREE)
            .running
    }

    /// The repository's data, for what the menus work out from it.
    fn shown<'a>(&self, cx: &'a App) -> Branches<'a> {
        Branches(self.repository.read(cx))
    }

    /// What deleting the branch `reference` deletes with it, or keeps; `None` if git won't delete
    /// it, as it, or a local branch tracking it, is checked out, or until that's known: the
    /// branches, the status and the worktrees have loaded. Until then, the dialog couldn't say
    /// which local branches go with a remote one.
    pub fn deletion(&self, reference: &str, cx: &App) -> Option<BranchDeletion> {
        self.shown(cx).deletion(reference)
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
        let delete = self.repository.entity().clone();
        self.asking_delete.set(true);
        let (deleting, cancelled) = (
            Rc::clone(&self.asking_delete),
            Rc::clone(&self.asking_delete),
        );
        open_delete_branch_dialog(
            deletion,
            move |deletion, _, cx| {
                deleting.set(false);
                let reference = deletion.reference().to_owned();
                delete.update(cx, |repository, cx| {
                    drop(repository.delete_branch(reference, cx))
                });
            },
            move |_, _| cancelled.set(false),
            window,
            cx,
        );
    }

    /// Pulls a local branch, by its full ref name: the checked-out one as the Pull button does,
    /// any other by fast-forwarding it to its upstream. The toolbar shows it running, and why it
    /// failed.
    pub fn pull(&mut self, reference: &str, cx: &mut Context<Self>) {
        let here = self.shown(cx).checked_out_here(reference);
        let branch = branch_name(reference).to_owned();
        self.repository.update(cx, |repository, cx| {
            if here {
                drop(repository.pull(cx));
            } else {
                drop(repository.pull_branch(branch, cx));
            }
        });
    }

    /// Whether the branch `reference` is being pulled: the checked-out one, or another.
    fn is_pulling(&self, reference: &str, cx: &App) -> bool {
        let shown = self.shown(cx);
        let name = if shown.checked_out_here(reference) {
            PULL
        } else {
            PULL_BRANCH
        };
        shown.0.operation_state(name).running
    }

    /// Merges the branch `reference`, by its full ref name, into the checked-out one, once the
    /// edits to a file on show are saved; if it stops at conflicts, opens the first conflicted
    /// file.
    pub fn merge(&mut self, reference: String, cx: &mut Context<Self>) {
        let Some(into) = self.shown(cx).target_branch() else {
            return;
        };
        self.integrate(
            move |repository, cx| repository.merge(Merge { reference, into }, cx),
            |outcome: &MergeOutcome| *outcome == MergeOutcome::Conflicts,
            cx,
        );
    }

    /// Rebases the checked-out branch onto the branch `reference`, by its full ref name, as
    /// merging does.
    pub fn rebase(&mut self, reference: String, cx: &mut Context<Self>) {
        let Some(branch) = self.shown(cx).target_branch() else {
            return;
        };
        self.integrate(
            move |repository, cx| repository.rebase(Rebase { reference, branch }, cx),
            |outcome: &RebaseOutcome| *outcome == RebaseOutcome::Conflicts,
            cx,
        );
    }

    /// Runs `run`, a merge or a rebase, once the edits to a file on show are saved; if it stops
    /// at conflicts (`conflicts` says), opens the first conflicted file.
    fn integrate<O: 'static>(
        &mut self,
        run: impl FnOnce(&mut Repository, &mut Context<Repository>) -> Done<O> + 'static,
        conflicts: fn(&O) -> bool,
        cx: &mut Context<Self>,
    ) {
        let id = self.repository.id(cx).clone();
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
                if this.repository.id(cx) != &id {
                    return None;
                }
                this.repository.update(cx, run)
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
                if this.repository.id(cx) != &id {
                    return;
                }
                // The status has reloaded by now (see `Repository::merge`).
                if let Some(file) = this.shown(cx).first_conflicted() {
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
        let shown = provider.shown(cx);
        let busy = shown.0.branches_changing();
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
        if let Some(head) = shown.head()
            && !is_checked_out(&head, &branch)
        {
            let may_integrate = shown.may_integrate();
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
        if shown.is_pullable(&branch) {
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
        menu = match shown.worktree_of(&branch) {
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
        let deletion = shown.deletion(&branch);
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

/// What the branch menus show of the repository.
const PARTS: &[Part] = &[
    Part::Status,
    Part::Operation,
    Part::Worktrees,
    Part::Refs,
    Part::Remotes,
    Part::Operations,
];

/// The repository's data, for what the branch menus work out from it.
struct Branches<'a>(&'a Repository);

impl Branches<'_> {
    fn head(&self) -> Option<Head> {
        self.0.status().data().map(|status| status.head.clone())
    }

    fn worktrees(&self) -> Option<&[Worktree]> {
        self.0.worktrees().data().map(|worktrees| &worktrees[..])
    }

    /// The branch to merge into, or to rebase: the one checked out; `None` with HEAD detached.
    fn target_branch(&self) -> Option<String> {
        match self.head()? {
            Head::Detached { .. } => None,
            Head::Branch { name, .. } | Head::Unborn { name } => Some(name),
        }
    }

    /// Whether a branch can be merged into the checked-out one, or that one rebased onto it.
    fn may_integrate(&self) -> bool {
        let conflicted = self
            .0
            .status()
            .data()
            .is_some_and(|status| status.counts.conflicted > 0);
        self.target_branch().is_some()
            && !conflicted
            && self.0.operation().data() == Some(&None)
            && !self.0.branches_changing()
    }

    /// The conflicted file to open first: one with markers left, if any is.
    fn first_conflicted(&self) -> Option<ChangedFile> {
        let status = self.0.status().data()?;
        first_to_resolve(&select_conflicted(status)).cloned()
    }

    /// Whether the local branch `reference` is checked out here.
    fn checked_out_here(&self, reference: &str) -> bool {
        self.head()
            .is_some_and(|head| is_checked_out(&head, reference))
    }

    /// Whether the local branch `reference` can be pulled from here: it tracks another, and is
    /// checked out here or in no worktree.
    fn is_pullable(&self, reference: &str) -> bool {
        let tracks = self.0.refs().data().is_some_and(|refs| {
            refs.iter()
                .any(|r| r.full_name == reference && r.upstream.is_some())
        });
        tracks
            && (self.checked_out_here(reference)
                || worktree_on(self.worktrees(), reference).is_none())
    }

    /// Whether the local branch `reference` is checked out, here or in another worktree.
    fn is_checked_out(&self, reference: &str) -> bool {
        self.checked_out_here(reference) || worktree_on(self.worktrees(), reference).is_some()
    }

    /// The other worktree the branch is checked out in, if one that can be opened is.
    fn worktree_of(&self, branch: &str) -> Option<Worktree> {
        worktree_on(self.worktrees(), branch)
            .filter(|worktree| can_open_worktree(worktree))
            .cloned()
    }

    /// See [`BranchMenuProvider::deletion`].
    fn deletion(&self, reference: &str) -> Option<BranchDeletion> {
        let all = self.0.refs().data()?;
        self.0.status().data()?;
        self.0.worktrees().data()?;
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
        let tracking = tracking_branches(all, reference);
        if tracking
            .iter()
            .any(|local| self.is_checked_out(&local.full_name))
        {
            return None;
        }
        // Taken to be there until the remotes have loaded, so it isn't played down.
        let on_remote = self
            .0
            .remotes()
            .data()
            .is_none_or(|remotes| remote_of(remotes, branch_name(reference)).is_some());
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
    use crate::data::QueryError;
    use crate::data::git::{GitTask, RepositoryGit};

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
            self.merges.borrow_mut().push(Merge {
                reference: reference.as_str().to_owned(),
                into: into.as_str().to_owned(),
            });
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
                    .ok_or(QueryError::msg("Unchanged."))
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
    fn forgets_the_branch_it_was_asking_about_deleting_in_another_repository(
        cx: &mut TestAppContext,
    ) {
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
}
