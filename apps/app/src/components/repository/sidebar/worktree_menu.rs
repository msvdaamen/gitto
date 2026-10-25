//! What can be done with a worktree in the sidebar. Ported from
//! `components/repository/sidebar/worktree-menu.tsx`.
//!
//! As with the stashes (see `stash_menu`), each row carries its menu, and the section shows the
//! failure itself: what's left of TS's `WorktreeMenu` is the menu's items, opening a worktree, and
//! what the section is told, here; the list (`RefList`) keeps which worktree's menu is open and
//! the worktree being asked about.

use std::rc::Rc;

use gitto_git::features::worktree::schema::Worktree;
use gitto_git::input::RepositoryId;
use gitto_ui::git::worktree::can_open_worktree;
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::{App, Window};

use super::sidebar_section::SectionFailure;
use crate::components::ui::{IconName, context_menu_item};
use crate::data::worktree::{WorktreeActions, worktree_actions};

/// Forgets why an action last failed on a repository, by its id.
type Dismiss = Rc<dyn Fn(&str, &mut App)>;

/// The menu of what can be done with a worktree. Opening it adds it to Gitto's repositories, as a
/// repository of its own, and shows it; not for the one on show, a bare one, which has no files,
/// or one whose folder is gone. Removing it deletes its folder, asking first, as its uncommitted
/// changes are lost; not the main worktree, nor the one on show. The worktrees' section shows
/// each running ([`is_busy`](Self::is_busy)), and why it failed beside it
/// ([`failure`](Self::failure)), as it does a worktree being added or opened from a branch's menu
/// (see `AddWorktreeDialog` and `branch_menu`).
#[derive(Clone)]
pub struct WorktreeMenu {
    actions: WorktreeActions,
}

impl WorktreeMenu {
    /// With the data layer's actions.
    pub fn new() -> Self {
        WorktreeMenu::with_actions(worktree_actions())
    }

    /// With `actions`: a test's.
    pub fn with_actions(actions: WorktreeActions) -> Self {
        WorktreeMenu { actions }
    }

    /// Whether a worktree is being added, opened or removed on the repository `id`.
    pub fn is_busy(&self, id: &str, cx: &App) -> bool {
        WorktreeActions::is_pending(id, cx)
    }

    /// Why removing, adding or opening a worktree failed on the repository `id`, if it did. One
    /// at a time, the removal's first, then the addition's: each can have failed, one after the
    /// other.
    pub fn failure(&self, id: &RepositoryId, cx: &App) -> SectionFailure {
        let remove = self.actions.remove.state(id, cx);
        let add = self.actions.add.state(id, cx);
        let (title, error, dismiss): (_, _, Dismiss) = if remove.error.is_some() {
            let remove_action = self.actions.remove.clone();
            (
                "Remove worktree",
                remove.error,
                Rc::new(move |id, cx| remove_action.dismiss(id, cx)),
            )
        } else if add.error.is_some() {
            let add_action = self.actions.add.clone();
            (
                "Add worktree",
                add.error,
                Rc::new(move |id, cx| add_action.dismiss(id, cx)),
            )
        } else {
            let open = self.actions.open.clone();
            (
                "Open worktree",
                open.state(id, cx).error,
                Rc::new(move |id, cx| open.dismiss(id, cx)),
            )
        };
        let id = id.clone();
        SectionFailure {
            title: title.into(),
            error,
            on_dismiss: Rc::new(move |_, cx| dismiss(&id, cx)),
        }
    }

    /// Opens `worktree` in Gitto, e.g. the one whose row was double-clicked, if it can be.
    pub fn open(&self, id: &RepositoryId, worktree: &Worktree, cx: &mut App) {
        if can_open_worktree(worktree) {
            let _ = self.actions.open.run(id, worktree.path.clone(), cx);
        }
    }

    /// Removes `worktree` on the repository `id`, once that's confirmed: not while another
    /// worktree action runs.
    pub fn remove(&self, id: &RepositoryId, worktree: &Worktree, cx: &mut App) {
        let _ = self.actions.remove.run(id, worktree.path.clone(), cx);
    }

    /// The menu's items for `worktree` on the repository `id`; `on_remove` asks before removing
    /// it.
    pub fn items(
        &self,
        menu: PopupMenu,
        id: &RepositoryId,
        worktree: &Worktree,
        on_remove: impl Fn(&mut Window, &mut App) + 'static,
        cx: &App,
    ) -> PopupMenu {
        let pending = self.is_busy(id, cx);
        let this = self.clone();
        let opened = (id.clone(), worktree.clone());
        menu.item(context_menu_item(
            IconName::FolderOpen,
            "Open worktree",
            !can_open_worktree(worktree) || pending,
            move |_, _, cx| this.open(&opened.0, &opened.1, cx),
        ))
        .item(context_menu_item(
            IconName::FolderX,
            "Remove worktree…",
            worktree.main || worktree.current || pending,
            move |_, window, cx| on_remove(window, cx),
        ))
    }
}

impl Default for WorktreeMenu {
    fn default() -> Self {
        WorktreeMenu::new()
    }
}
