//! What can be done with a worktree in the sidebar. Ported from
//! `components/repository/sidebar/worktree-menu.tsx`.
//!
//! As with the stashes (see `stash_menu`), each row carries its menu, and the section shows the
//! failure itself: what's left of TS's `WorktreeMenu` is the menu's items, opening a worktree, and
//! what the section is told, here; the list (`RefList`) keeps which worktree's menu is open and
//! the worktree being asked about.

use std::rc::Rc;

use gitto_git::features::worktree::schema::Worktree;
use gitto_ui_logic::git::worktree::can_open_worktree;
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::{App, Entity, Window};

use super::sidebar_section::SectionFailure;
use gitto_app_data::data::Change;
use gitto_app_data::data::Repository;
use gitto_app_kit::ui::{IconName, context_menu_item};

/// The menu of what can be done with a worktree. Opening it adds it to Gitto's repositories, as a
/// repository of its own, and shows it; not for the one on show, a bare one, which has no files,
/// or one whose folder is gone. Removing it deletes its folder, asking first, as its uncommitted
/// changes are lost; not the main worktree, nor the one on show. The worktrees' section shows
/// each running ([`is_busy`](Self::is_busy)), and why it failed beside it
/// ([`failure`](Self::failure)), as it does a worktree being added or opened from a branch's menu
/// (see `AddWorktreeDialog` and `branch_menu`).
pub struct WorktreeMenu;

impl WorktreeMenu {
    /// Whether a worktree is being added, opened or removed on `repository`.
    pub fn is_busy(repository: &Repository) -> bool {
        repository.worktrees_changing()
    }

    /// Why removing, adding or opening a worktree failed on `repository`, if it did. One at a
    /// time, the removal's first, then the addition's: each can have failed, one after the other.
    pub fn failure(repository: &Entity<Repository>, cx: &App) -> SectionFailure {
        let shown = repository.read(cx);
        let remove = shown.change_state(Change::RemoveWorktree);
        let add = shown.change_state(Change::AddWorktree);
        let (title, error, name) = if remove.error.is_some() {
            ("Remove worktree", remove.error, Change::RemoveWorktree)
        } else if add.error.is_some() {
            ("Add worktree", add.error, Change::AddWorktree)
        } else {
            let open = shown.change_state(Change::OpenWorktree);
            ("Open worktree", open.error, Change::OpenWorktree)
        };
        let repository = repository.clone();
        SectionFailure {
            title: title.into(),
            error,
            on_dismiss: Rc::new(move |_, cx| {
                repository.update(cx, |repository, cx| repository.dismiss(name, cx))
            }),
        }
    }

    /// Opens `worktree` in Gitto, e.g. the one whose row was double-clicked, if it can be.
    pub fn open(repository: &Entity<Repository>, worktree: &Worktree, cx: &mut App) {
        if can_open_worktree(worktree) {
            let path = worktree.path.clone();
            repository.update(cx, |repository, cx| {
                drop(repository.open_worktree(path, cx))
            });
        }
    }

    /// Removes `worktree` from `repository`, once that's confirmed: not while another worktree
    /// action runs.
    pub fn remove(repository: &Entity<Repository>, worktree: &Worktree, cx: &mut App) {
        let path = worktree.path.clone();
        repository.update(cx, |repository, cx| {
            drop(repository.remove_worktree(path, cx))
        });
    }

    /// The menu's items for `worktree` of `repository`; `on_remove` asks before removing it.
    pub fn items(
        menu: PopupMenu,
        repository: &Entity<Repository>,
        worktree: &Worktree,
        on_remove: impl Fn(&mut Window, &mut App) + 'static,
        cx: &App,
    ) -> PopupMenu {
        let pending = WorktreeMenu::is_busy(repository.read(cx));
        let opened = (repository.clone(), worktree.clone());
        menu.item(context_menu_item(
            IconName::FolderOpen,
            "Open worktree",
            !can_open_worktree(worktree) || pending,
            move |_, _, cx| WorktreeMenu::open(&opened.0, &opened.1, cx),
        ))
        .item(context_menu_item(
            IconName::FolderX,
            "Remove worktree…",
            worktree.main || worktree.current || pending,
            move |_, window, cx| on_remove(window, cx),
        ))
    }
}
