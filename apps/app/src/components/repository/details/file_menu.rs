//! The menu of what can be done with an uncommitted file. Ported from
//! `components/repository/details/file-menu.tsx`.

use std::rc::Rc;

use gitto_git::features::diff::schema::ChangedFile;
use gitto_git::schema::FileStatus;
use gitto_ui::git::changes::is_nested_repository;
use gpui_kit::{App, Context, DismissEvent, Window};

use crate::components::ui::{IconName, PopupMenu, context_menu_item};

/// Told which file's menu is open, by its path; `None` once it's closed.
pub type OnOpenFor = Rc<dyn Fn(Option<String>, &mut App)>;
/// Asks about discarding a file's changes.
pub type OnDiscard = Rc<dyn Fn(&ChangedFile, &mut Window, &mut App)>;

/// The menu of what can be done with an uncommitted file, `file`, opened by right-clicking its
/// row: discarding its changes, which `on_discard` asks about first, as they're lost. Not for a
/// conflicted file, which is for resolving, nor a repository inside this one, whose files aren't
/// this one's; nor while files are being staged or discarded (`busy`). `on_open_for` is told the
/// file's path while the menu's open, and `None` once it's closed, to show which one it's for.
pub fn file_menu(
    menu: PopupMenu,
    file: &ChangedFile,
    busy: bool,
    on_open_for: OnOpenFor,
    on_discard: OnDiscard,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    on_open_for(Some(file.path.clone()), cx);
    cx.subscribe_self(move |_, _: &DismissEvent, cx| on_open_for(None, cx))
        .detach();
    let disabled = busy || file.status == FileStatus::Conflicted || is_nested_repository(file);
    let file = file.clone();
    menu.item(context_menu_item(
        IconName::Undo2,
        "Discard changes…",
        disabled,
        move |_, window, cx| on_discard(&file, window, cx),
    ))
}
