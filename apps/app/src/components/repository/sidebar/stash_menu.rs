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
use gitto_git::input::RepositoryId;
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::{App, Window};

use super::sidebar_section::SectionFailure;
use crate::components::ui::{IconName, context_menu_item};
use crate::data::stash::{StashActions, stash_actions};

/// The menu of what can be done with a stash: popping it puts its changes back and drops it;
/// deleting it asks first, as its changes are lost. Like the toolbar's, one at a time. The
/// stashes' section shows each running ([`is_busy`](Self::is_busy)), and why it failed beside it
/// ([`failure`](Self::failure)).
#[derive(Clone)]
pub struct StashMenu {
    actions: StashActions,
}

impl StashMenu {
    /// With the data layer's actions.
    pub fn new() -> Self {
        StashMenu::with_actions(stash_actions())
    }

    /// With `actions`: a test's.
    pub fn with_actions(actions: StashActions) -> Self {
        StashMenu { actions }
    }

    pub fn actions(&self) -> &StashActions {
        &self.actions
    }

    /// Whether a stash is being popped or deleted from the sidebar on the repository `id`.
    pub fn is_busy(&self, id: &RepositoryId, cx: &App) -> bool {
        self.actions.pop_picked.state(id, cx).running || self.actions.drop.state(id, cx).running
    }

    /// Why popping or deleting a stash failed on the repository `id`, if it did. One at a time,
    /// the deletion's first: both can have failed, one after the other.
    pub fn failure(&self, id: &RepositoryId, cx: &App) -> SectionFailure {
        let drop = self.actions.drop.state(id, cx);
        let (title, error, dismiss) = if drop.error.is_some() {
            ("Delete stash", drop.error, self.actions.drop.clone())
        } else {
            let pop = self.actions.pop_picked.state(id, cx);
            ("Pop stash", pop.error, self.actions.pop_picked.clone())
        };
        let id = id.clone();
        SectionFailure {
            title: title.into(),
            error,
            on_dismiss: Rc::new(move |_, cx| dismiss.dismiss(&id, cx)),
        }
    }

    /// The menu's items for `stash` on the repository `id`. Popping it isn't possible while
    /// `poppable` says it can't be (conflicts, say), nor either while a stash action runs;
    /// `on_delete` asks before deleting it.
    pub fn items(
        &self,
        menu: PopupMenu,
        id: &RepositoryId,
        stash: &Stash,
        poppable: bool,
        on_delete: impl Fn(&mut Window, &mut App) + 'static,
        cx: &App,
    ) -> PopupMenu {
        let pending = StashActions::is_pending(id, cx);
        let pop = self.actions.pop_picked.clone();
        let popped = (id.clone(), stash.sha.clone());
        menu.item(context_menu_item(
            IconName::ArchiveRestore,
            "Pop stash",
            !poppable || pending,
            move |_, _, cx| {
                let (id, sha) = popped.clone();
                let _ = pop.run(&id, sha, cx);
            },
        ))
        .item(context_menu_item(
            IconName::Trash,
            "Delete stash…",
            pending,
            move |_, window, cx| on_delete(window, cx),
        ))
    }

    /// Deletes `stash` on the repository `id`, once that's confirmed: not while another stash
    /// action runs.
    pub fn drop(&self, id: &RepositoryId, stash: &Stash, cx: &mut App) {
        let _ = self.actions.drop.run(id, stash.sha.clone(), cx);
    }
}

impl Default for StashMenu {
    fn default() -> Self {
        StashMenu::new()
    }
}
