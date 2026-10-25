//! The menu of what can be done with a commit. Ported from
//! `components/repository/history/commit-menu.tsx`.

use gitto_git::features::operation::schema::Operation;
use gitto_git::features::status::schema::{Head, StatusSummary, Uncommitted};
use gitto_git::input::RepositoryId;
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::{
    AppContext as _, Context, Entity, EventEmitter, SharedString, Subscription, Window,
};

use super::reword_dialog::{RewordDialog, RewordEvent, RewordMutation};
use crate::components::repository::row_menu::RowMenu;
use crate::components::ui::{IconName, context_menu_item};
use crate::data::progress::operation_in_progress_query;
use crate::data::status::status_query;
use crate::data::{Opaque, Query};

/// The menu of what can be done with a commit, opened by right-clicking its row in the history
/// (see `HistoryOption::menu`). Only the last commit of the checked-out branch has one, to edit
/// its message (see [`RewordDialog`]); not while an operation (a merge, a rebase…) is under way,
/// as git won't amend then. A branch's label in the row opens the branch's menu instead, as the
/// innermost element's menu opens. An entity, kept by the history: it says which commit's menu
/// is open, for its row to show, and passes on the dialog's [`RewordEvent`]s.
pub struct CommitMenu {
    repository_id: RepositoryId,
    status: Query<Opaque<Uncommitted>, StatusSummary>,
    operation: Query<Option<Operation>>,
    menu: RowMenu,
    reword: Entity<RewordDialog>,
    _subscription: Subscription,
}

impl EventEmitter<RewordEvent> for CommitMenu {}

impl CommitMenu {
    pub fn new(repository_id: RepositoryId, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let reword = cx.new(|cx| RewordDialog::new(repository_id.clone(), window, cx));
        CommitMenu::with_dialog(repository_id, reword, cx)
    }

    /// With the dialog rewording commits with `reword`: a test's.
    pub(crate) fn with_reword(
        repository_id: RepositoryId,
        reword: Entity<RewordMutation>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let dialog =
            cx.new(|cx| RewordDialog::with_reword(repository_id.clone(), reword, window, cx));
        CommitMenu::with_dialog(repository_id, dialog, cx)
    }

    fn with_dialog(
        repository_id: RepositoryId,
        reword: Entity<RewordDialog>,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscription = cx.subscribe(&reword, |_, _, event: &RewordEvent, cx| {
            cx.emit(event.clone())
        });
        CommitMenu {
            status: status_query(&repository_id, cx),
            operation: operation_in_progress_query(&repository_id, cx),
            repository_id,
            menu: RowMenu::default(),
            reword,
            _subscription: subscription,
        }
    }

    /// Of another repository: a commit of one isn't one of another, so its menu and the dialog
    /// close.
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
        self.repository_id = repository_id.clone();
        self.menu.close(cx);
        self.reword.update(cx, |dialog, cx| {
            dialog.set_repository(repository_id, window, cx)
        });
        cx.notify();
    }

    pub fn dialog(&self) -> &Entity<RewordDialog> {
        &self.reword
    }

    /// The last commit of the checked-out branch; none with HEAD detached, or before the first.
    pub fn branch_tip(&self) -> Option<String> {
        match self.status.data()?.head {
            Head::Branch { sha, .. } => Some(sha),
            Head::Detached { .. } | Head::Unborn { .. } => None,
        }
    }

    /// Whether the commit `sha` has a menu: it's the last of the checked-out branch.
    pub fn has_menu(&self, sha: &str) -> bool {
        self.branch_tip().as_deref() == Some(sha)
    }

    /// The SHA of the commit whose menu is open, if any, for its row to show which one it's for.
    pub fn open_for(&self) -> Option<&SharedString> {
        self.menu.open_for()
    }

    /// Begins editing the message of the commit `sha`.
    pub fn reword(&mut self, sha: String, window: &mut Window, cx: &mut Context<Self>) {
        self.reword
            .update(cx, |dialog, cx| dialog.open(sha, window, cx));
    }

    /// The menu of the commit `sha` (see the type's doc): for its row's `.context_menu(…)`.
    pub fn build_menu(
        this: &Entity<Self>,
        sha: SharedString,
        menu: PopupMenu,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        if !this.read(cx).has_menu(&sha) {
            return menu;
        }
        RowMenu::track(
            this.downgrade(),
            sha.clone(),
            |commit: &mut CommitMenu| &mut commit.menu,
            cx,
        );
        // Not while an operation is under way; also until it's known whether one is.
        let disabled = this.read(cx).operation.data() != Some(None);
        let this = this.clone();
        menu.item(context_menu_item(
            IconName::PenLine,
            "Edit message…",
            disabled,
            move |_, window, cx| {
                let sha = sha.to_string();
                this.update(cx, |this, cx| this.reword(sha, window, cx));
            },
        ))
    }
}
