//! What can be done with a stash in the sidebar. Ported from
//! `components/repository/sidebar/stash-menu.tsx`.
//!
//! TS's `StashMenu` wrapped the stashes' section: a `RowMenu` opening off the row right-clicked,
//! the failure popover beside the section, and the dialog asking before deleting. In GPUI each
//! row carries its menu (see `row_menu`), the section shows the failure itself
//! (`SidebarSection::failure`), and the dialog is opened when asked for: what's left is the menu's
//! items, and what the section is told (busy, the failure), here; the list (`RefList`) keeps
//! which stash's menu is open and the stash being asked about.

use std::rc::Rc;

use gitto_git::features::stash::schema::Stash;
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::{App, Entity, Window};

use super::sidebar_section::SectionFailure;
use gitto_app_data::data::Repository;
use gitto_app_data::data::{Change, PopFrom};
use gitto_app_kit::ui::{IconName, context_menu_item};

/// The menu of what can be done with a stash: popping it puts its changes back and drops it;
/// deleting it asks first, as its changes are lost. Like the toolbar's, one at a time. The
/// stashes' section shows each running ([`is_busy`](Self::is_busy)), and why it failed beside it
/// ([`failure`](Self::failure)).
pub struct StashMenu;

impl StashMenu {
    /// Whether a stash is being popped or deleted from the sidebar on `repository`.
    pub fn is_busy(repository: &Repository) -> bool {
        repository.is_running(&[Change::Pop(PopFrom::Picked), Change::DropStash])
    }

    /// Why popping or deleting a stash failed on `repository`, if it did. One at a time, the
    /// deletion's first: both can have failed, one after the other.
    pub fn failure(repository: &Entity<Repository>, cx: &App) -> SectionFailure {
        let shown = repository.read(cx);
        let drop = shown.change_state(Change::DropStash);
        let (title, error, name) = if drop.error.is_some() {
            ("Delete stash", drop.error, Change::DropStash)
        } else {
            let pop = shown.change_state(Change::Pop(PopFrom::Picked));
            ("Pop stash", pop.error, Change::Pop(PopFrom::Picked))
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

    /// The menu's items for `stash` on `repository`. Popping it isn't possible while `poppable`
    /// says it can't be (conflicts, say), nor either while a stash action runs; `on_delete` asks
    /// before deleting it.
    pub fn items(
        menu: PopupMenu,
        repository: &Entity<Repository>,
        stash: &Stash,
        poppable: bool,
        on_delete: impl Fn(&mut Window, &mut App) + 'static,
        cx: &App,
    ) -> PopupMenu {
        let pending = repository.read(cx).stashes_changing();
        let pop = repository.clone();
        let sha = stash.sha.clone();
        menu.item(context_menu_item(
            IconName::ArchiveRestore,
            "Pop stash",
            !poppable || pending,
            move |_, _, cx| {
                let sha = sha.clone();
                pop.update(cx, |repository, cx| {
                    drop(repository.pop_stash(PopFrom::Picked, sha, cx))
                });
            },
        ))
        .item(context_menu_item(
            IconName::Trash,
            "Delete stash…",
            pending,
            move |_, window, cx| on_delete(window, cx),
        ))
    }

    /// Deletes `stash` on `repository`, once that's confirmed: not while another stash action
    /// runs.
    pub fn drop(repository: &Entity<Repository>, stash: &Stash, cx: &mut App) {
        let sha = stash.sha.clone();
        repository.update(cx, |repository, cx| drop(repository.drop_stash(sha, cx)));
    }
}
