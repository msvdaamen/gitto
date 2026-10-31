//! Asks before deleting a stash. Ported from
//! `components/repository/sidebar/drop-stash-dialog.tsx`.

use gitto_git::features::stash::schema::Stash;
use gpui_kit::component::button::ButtonVariant;
use gpui_kit::{App, Window};

use gitto_app_kit::ui::{ConfirmDialog, DialogId, IconName, Tone};

/// What deleting `stash` does, as the dialog says it.
pub fn drop_stash_description(stash: &Stash) -> String {
    format!(
        "\"{}\" is deleted, and the changes in it are lost.",
        stash.message
    )
}

/// Asks before deleting `stash`, whose changes are lost with it; `on_drop` deletes it. Esc, or
/// clicking outside, cancels, which `on_cancel` is told.
pub fn open_drop_stash_dialog(
    stash: &Stash,
    on_drop: impl Fn(&mut Window, &mut App) + 'static,
    on_cancel: impl Fn(&mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) -> DialogId {
    ConfirmDialog::new(IconName::Trash, Tone::Coral, "Delete stash?")
        .description(drop_stash_description(stash))
        .action("Delete", ButtonVariant::Danger, on_drop)
        .on_cancel(on_cancel)
        .open(window, cx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn says_the_changes_in_the_stash_are_lost() {
        let stash = Stash {
            sha: "a".repeat(40),
            base: "abc".into(),
            message: "On main: older".into(),
            created_at: 1,
        };
        assert_eq!(
            drop_stash_description(&stash),
            "\"On main: older\" is deleted, and the changes in it are lost."
        );
    }
}
