//! Asking before marking a file resolved with conflict markers left in it. Ported from
//! `markers-dialog.tsx`.

use gpui_kit::component::button::ButtonVariant;
use gpui_kit::{App, Window};

use gitto_app_kit::ui::{ConfirmDialog, IconName, Tone};

/// The name a path ends with.
pub fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// What the dialog says of the conflicts left: `left` of them, as the view reads them; none when
/// its markers can't be read as conflicts.
pub fn markers_description(left: usize) -> String {
    let left = match left {
        0 => "It still has conflict markers.".to_owned(),
        1 => "It still has 1 conflict.".to_owned(),
        left => format!("It still has {left} conflicts."),
    };
    format!(
        "{left} Mark it resolved only if they belong in it, like a test's fixture of them: \
         they're staged as they are."
    )
}

/// Asks before marking `path` resolved with conflict markers left in it, which is only right when
/// they belong in it, like a test's fixture of them: they're staged as they are. Escape, or
/// clicking outside, cancels; `confirm` runs on Mark resolved.
pub fn open_markers_dialog(
    path: &str,
    left: usize,
    confirm: impl Fn(&mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    ConfirmDialog::new(
        IconName::TriangleAlert,
        Tone::Amber,
        format!(
            "Mark {} resolved with its conflict markers?",
            file_name(path)
        ),
    )
    .description(markers_description(left))
    .action("Mark resolved", ButtonVariant::Primary, confirm)
    .open(window, cx);
}
