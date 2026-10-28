//! The menu of what can be done with a commit. Ported from
//! `components/repository/history/commit-menu.tsx`.

use gitto_git::features::status::schema::Head;
use gitto_git::input::RepositoryId;
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::{
    App, AppContext as _, Context, Entity, EventEmitter, SharedString, Subscription, Window,
};

use super::reword_dialog::{RewordDialog, RewordEvent};
use crate::components::repository::row_menu::RowMenu;
use gitto_app_data::data::{Part, Shown};
use gitto_app_kit::ui::{IconName, context_menu_item};

/// What the commit menu shows of the repository: HEAD, and whether an operation is under way.
const PARTS: &[Part] = &[Part::Status, Part::Operation];

/// The menu of what can be done with a commit, opened by right-clicking its row in the history
/// (see `HistoryOption::menu`). Only the last commit of the checked-out branch has one, to edit
/// its message (see [`RewordDialog`]); not while an operation (a merge, a rebase…) is under way,
/// as git won't amend then. A branch's label in the row opens the branch's menu instead, as the
/// innermost element's menu opens. An entity, kept by the history: it says which commit's menu
/// is open, for its row to show, and passes on the dialog's [`RewordEvent`]s.
pub struct CommitMenu {
    repository: Shown,
    menu: RowMenu,
    reword: Entity<RewordDialog>,
    _subscription: Subscription,
}

impl EventEmitter<RewordEvent> for CommitMenu {}

impl CommitMenu {
    pub fn new(repository_id: RepositoryId, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let reword = cx.new(|cx| RewordDialog::new(repository_id.clone(), window, cx));
        let subscription = cx.subscribe(&reword, |_, _, event: &RewordEvent, cx| {
            cx.emit(event.clone())
        });
        CommitMenu {
            repository: Shown::new(&repository_id, PARTS, cx),
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
        if !self.repository.set(&repository_id, cx) {
            return;
        }
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
    pub fn branch_tip(&self, cx: &App) -> Option<String> {
        match &self.repository.read(cx).status().data()?.summary.head {
            Head::Branch { sha, .. } => Some(sha.clone()),
            Head::Detached { .. } | Head::Unborn { .. } => None,
        }
    }

    /// Whether the commit `sha` has a menu: it's the last of the checked-out branch.
    pub fn has_menu(&self, sha: &str, cx: &App) -> bool {
        self.branch_tip(cx).as_deref() == Some(sha)
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
        if !this.read(cx).has_menu(&sha, cx) {
            return menu;
        }
        RowMenu::track(
            this.downgrade(),
            sha.clone(),
            |commit: &mut CommitMenu| &mut commit.menu,
            cx,
        );
        // Not while an operation is under way; also until it's known whether one is.
        let operation = this.read(cx).repository.read(cx).operation().data();
        let disabled = operation != Some(&None);
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
