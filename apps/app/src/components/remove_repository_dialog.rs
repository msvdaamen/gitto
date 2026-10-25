//! Asks before removing a repository from Gitto. Ported from
//! `components/remove-repository-dialog.tsx`.

use gitto_repository::Repository;
use gpui_kit::component::button::ButtonVariant;
use gpui_kit::{App, Window};

use gitto_app_kit::ui::{ConfirmDialog, IconName, Tone};

/// Asks before removing `repository` from Gitto, which leaves its folder as it is; `on_remove`
/// removes it. Escape, or clicking outside, cancels.
pub fn open_remove_repository_dialog(
    repository: &Repository,
    on_remove: impl Fn(&mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    ConfirmDialog::new(
        IconName::X,
        Tone::Amber,
        format!("Remove {}?", repository.name),
    )
    .description(format!(
        "Its tab is closed and it's taken off Gitto's list. The folder {} is kept as it is.",
        repository.path
    ))
    .action("Remove", ButtonVariant::Primary, on_remove)
    .open(window, cx);
}
