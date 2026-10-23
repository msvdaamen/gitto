//! Which item's context menu is open, and closing it once it's no longer of what's on show.
//! Ported from `components/repository/row-menu.tsx`.
//!
//! TS's `RowMenu` was one menu for a whole list, opened off the element right-clicked by its
//! `data-*` attribute, working out whose it was when one item's element held another's (a branch's
//! label in a commit's row). In GPUI each element carries its own menu
//! (`.context_menu(|menu, window, cx| …)`, see `components::ui::context_menu`): the innermost
//! element's opens, as GPUI Kit's menus take the right-click innermost first, and a list only
//! renders the rows in view, so there's no cost per item to save. What's left of `RowMenu` is
//! this: telling the list which item's menu is open, to show it (`onOpenFor`), and closing that
//! menu once the repository changes, as another repository's item isn't one of this one's
//! (`CloseOnChange`).

use gpui_kit::component::menu::PopupMenu;
use gpui_kit::{App, Context, DismissEvent, SharedString, WeakEntity};

/// The item whose menu is open, by its key, and that menu. An owner (a list, or the branch menu
/// they share) keeps one, and [`track`](Self::track)s each menu it builds.
#[derive(Default)]
pub struct RowMenu {
    open: Option<(SharedString, WeakEntity<PopupMenu>)>,
}

impl RowMenu {
    /// The key of the item whose menu is open, if any.
    pub fn open_for(&self) -> Option<&SharedString> {
        self.open
            .as_ref()
            .filter(|(_, menu)| menu.upgrade().is_some())
            .map(|(key, _)| key)
    }

    /// Takes the menu being built in `cx` as the one open for the item `key`, until it's
    /// dismissed: `owner`'s `RowMenu` (which `field` picks) says so, and `owner` is notified each
    /// time that changes. Call it from the menu's builder.
    pub fn track<V: 'static>(
        owner: WeakEntity<V>,
        key: SharedString,
        field: fn(&mut V) -> &mut RowMenu,
        cx: &mut Context<PopupMenu>,
    ) {
        let menu = cx.entity().downgrade();
        let _ = owner.update(cx, |owner, cx| {
            field(owner).open = Some((key, menu.clone()));
            cx.notify();
        });
        cx.subscribe_self(move |_, _: &DismissEvent, cx| {
            let menu = menu.clone();
            let _ = owner.update(cx, |owner, cx| {
                let row_menu = field(owner);
                if row_menu
                    .open
                    .as_ref()
                    .is_some_and(|(_, open)| *open == menu)
                {
                    row_menu.open = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Closes the open menu, if any: e.g. once another repository is on show.
    pub fn close(&mut self, cx: &mut App) {
        if let Some((_, menu)) = self.open.take()
            && let Some(menu) = menu.upgrade()
        {
            menu.update(cx, |_, cx| cx.emit(DismissEvent));
        }
    }
}
