//! Asks before removing a worktree. Ported from
//! `components/repository/sidebar/remove-worktree-dialog.tsx`.

use gitto_git::features::worktree::schema::Worktree;
use gitto_ui::git::status::branch_name;
use gpui_kit::component::button::ButtonVariant;
use gpui_kit::{App, Window};

use crate::components::ui::{ConfirmDialog, IconName, Tone};

/// What removing `worktree` does, as the dialog says it: its folder goes, with any uncommitted
/// changes in it; its branch, if it's on one, is kept.
pub fn remove_worktree_description(worktree: &Worktree) -> String {
    let branch = worktree
        .branch
        .as_deref()
        .filter(|branch| !branch.is_empty())
        .map(|branch| format!(" The branch {} is kept.", branch_name(branch)))
        .unwrap_or_default();
    format!(
        "The folder {} is deleted, and any uncommitted changes in it are lost.{branch}",
        worktree.path
    )
}

/// Asks before removing `worktree`, whose folder is deleted with any uncommitted changes in it;
/// `on_remove` removes it. Esc, or clicking outside, cancels, which `on_cancel` is told.
pub fn open_remove_worktree_dialog(
    worktree: &Worktree,
    on_remove: impl Fn(&mut Window, &mut App) + 'static,
    on_cancel: impl Fn(&mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    ConfirmDialog::new(IconName::FolderX, Tone::Coral, "Remove worktree?")
        .description(remove_worktree_description(worktree))
        .action("Remove", ButtonVariant::Primary, on_remove)
        .on_cancel(on_cancel)
        .open(window, cx);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn worktree(branch: Option<&str>) -> Worktree {
        Worktree {
            path: "/w/repo-feature".into(),
            name: "repo-feature".into(),
            head: Some("abc".into()),
            branch: branch.map(Into::into),
            main: false,
            bare: false,
            current: false,
            locked: None,
            prunable: None,
        }
    }

    #[test]
    fn says_the_folder_is_deleted_and_the_branch_kept() {
        assert_eq!(
            remove_worktree_description(&worktree(Some("refs/heads/feature"))),
            "The folder /w/repo-feature is deleted, and any uncommitted changes in it are lost. \
             The branch feature is kept."
        );
        assert_eq!(
            remove_worktree_description(&worktree(None)),
            "The folder /w/repo-feature is deleted, and any uncommitted changes in it are lost."
        );
    }
}
