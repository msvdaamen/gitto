//! Asking what to do with edits that couldn't be saved. Ported from `unsaved-changes-dialog.tsx`.

use std::rc::Rc;

use gpui_kit::component::button::ButtonVariant;
use gpui_kit::{App, Window};

use super::markers_dialog::file_name;
use crate::components::ui::{ConfirmDialog, IconName, Tone};

/// What to do with edits that couldn't be saved, before going on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnsavedChoice {
    Save,
    Discard,
    Cancel,
}

/// Asks what to do with `path`'s edits that couldn't be saved, before what the user asked for
/// (like closing the file) goes on: save them anyway, drop them, or stay (`choose`). Escape, or
/// clicking outside, stays. Saving overwrites the file if it `changed_on_disk` since; `reason` says
/// why they weren't saved.
pub fn open_unsaved_changes_dialog(
    path: &str,
    reason: &str,
    changed_on_disk: bool,
    choose: impl Fn(UnsavedChoice, &mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    let choose = Rc::new(choose);
    let then = if changed_on_disk {
        "Saving replaces the file with your version."
    } else {
        "Your latest changes aren't saved yet."
    };
    let (cancel, discard, save) = (Rc::clone(&choose), Rc::clone(&choose), choose);
    ConfirmDialog::new(
        IconName::TriangleAlert,
        Tone::Amber,
        format!("Save your changes to {}?", file_name(path)),
    )
    .description(format!("{reason} {then}"))
    .action("Discard", ButtonVariant::Default, move |window, cx| {
        discard(UnsavedChoice::Discard, window, cx)
    })
    .action(
        if changed_on_disk { "Overwrite" } else { "Save" },
        ButtonVariant::Primary,
        move |window, cx| save(UnsavedChoice::Save, window, cx),
    )
    .on_cancel(move |window, cx| cancel(UnsavedChoice::Cancel, window, cx))
    .open(window, cx);
}
