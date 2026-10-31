//! Context menus. Ported from `components/ui/context-menu.tsx`.
//!
//! A context menu is GPUI Kit's: `.context_menu(|menu, window, cx| …)` on the element it's of
//! (`ContextMenuExt`), opened where it's right-clicked, with its keyboard navigation, dismissal and
//! focus. Its items are [`context_menu_item`]s: an icon and a label, the menu closing once one's
//! chosen. Menus share one look, the theme's popover.

use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::{App, ClickEvent, SharedString, Window, px};

pub use gpui_kit::component::menu::{ContextMenuExt, PopupMenu};

use super::icons::{Icon, IconName};

/// A context menu, at least as wide as TS's (180px), whatever its items' labels.
pub fn menu_frame(menu: PopupMenu) -> PopupMenu {
    menu.min_w(px(180.))
}

/// One of a context menu's actions, which `on_select` runs; the menu closes once it's chosen.
pub fn context_menu_item(
    icon: IconName,
    label: impl Into<SharedString>,
    disabled: bool,
    on_select: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> PopupMenuItem {
    PopupMenuItem::new(label)
        .icon(Icon::new(icon))
        .disabled(disabled)
        .on_click(on_select)
}
