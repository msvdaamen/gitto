//! Asks before discarding changes. Ported from
//! `components/repository/details/discard-dialog.tsx`.

use std::cell::RefCell;
use std::rc::Rc;

use gitto_git::features::diff::schema::ChangedFile;
use gitto_git::features::staging::discard::DiscardSide;
use gitto_git::schema::FileStatus;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::prelude::*;
use gpui_kit::{App, Window, div};

use super::changed_file_list::name_and_folder;
use crate::components::ui::dialog::CONFIRM_WIDTH;
use crate::components::ui::{DialogHeading, IconName, Tone};
use crate::data::staging::DiscardTarget;
use crate::state::lingering::Lingering;

/// What the dialog asks about: the changes to discard and, for a file's unstaged ones, whether it
/// has staged ones too, `also_staged`, which it's put back as; else as the last commit has it.
#[derive(Debug, Clone, PartialEq)]
pub enum DiscardPrompt {
    All,
    File {
        side: DiscardSide,
        file: ChangedFile,
        also_staged: bool,
    },
}

impl DiscardPrompt {
    /// The changes to discard, as the data layer takes them.
    pub fn target(&self) -> DiscardTarget {
        match self {
            DiscardPrompt::All => DiscardTarget::All,
            DiscardPrompt::File { side, file, .. } => DiscardTarget::File {
                side: *side,
                file: file.clone(),
            },
        }
    }

    /// The dialog's title: "Discard all changes?", or the file's, by its name.
    pub fn title(&self) -> String {
        match self {
            DiscardPrompt::All => "Discard all changes?".to_owned(),
            DiscardPrompt::File { file, .. } => {
                let (name, _) = name_and_folder(&file.path);
                format!("Discard changes to {name}?")
            }
        }
    }

    /// What discarding the changes does, as the dialog says it.
    pub fn describe(&self) -> String {
        let (side, file, also_staged) = match self {
            DiscardPrompt::All => {
                return "Every staged and unstaged change is lost, and untracked files are deleted. Ignored files are kept, and so are submodules' changes.".to_owned();
            }
            DiscardPrompt::File {
                side,
                file,
                also_staged,
            } => (*side, file, *also_staged),
        };
        let path = &file.path;
        if side == DiscardSide::Unstaged {
            // Added or renamed, the file was only marked to be added (`add -N`), and isn't staged.
            if file.status == FileStatus::Untracked {
                return format!("{path} isn't tracked, so it's deleted.");
            }
            if file.status == FileStatus::Added {
                return format!("{path} isn't staged yet, so it's deleted.");
            }
            // What's left of it: what's staged, which is what the last commit has if nothing is.
            let kept = if also_staged {
                "as it's staged"
            } else {
                "as the last commit has it"
            };
            if let (FileStatus::Renamed, Some(orig_path)) = (file.status, &file.orig_path) {
                return format!("{path} is deleted, and {orig_path} put back {kept}.");
            }
            if file.status == FileStatus::Deleted {
                return format!("{path} is put back {kept}.");
            }
            let staged = if also_staged {
                " What's staged of it is kept."
            } else {
                ""
            };
            return format!("The unstaged changes to {path} are lost.{staged}");
        }
        // A copy is new too: the file it's a copy of is left as it is.
        if matches!(file.status, FileStatus::Added | FileStatus::Copied) {
            return format!(
                "{path} is new, so it's deleted, with its staged and unstaged changes."
            );
        }
        if let (FileStatus::Renamed, Some(orig_path)) = (file.status, &file.orig_path) {
            return format!(
                "{path} is deleted, and {orig_path} put back as the last commit has it: the staged and unstaged changes are lost."
            );
        }
        format!(
            "{path} is put back as the last commit has it: its staged and unstaged changes are lost."
        )
    }
}

/// Asks before discarding changes, which are lost: the ones `prompt` says, read again whenever
/// the dialog is drawn, so it asks about them as they are now (a file staged in a terminal
/// meanwhile, say); still named once they're gone, as the dialog closes. Escape, a click outside,
/// or Cancel runs `on_cancel`; Discard runs `on_discard` with the changes asked about.
pub fn open_discard_dialog(
    prompt: impl Fn(&App) -> Option<DiscardPrompt> + 'static,
    on_cancel: impl Fn(&mut Window, &mut App) + 'static,
    on_discard: impl Fn(DiscardPrompt, &mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    let prompt = Rc::new(prompt);
    let on_cancel = Rc::new(on_cancel);
    let on_discard = Rc::new(on_discard);
    let shown = Rc::new(RefCell::new(Lingering::default()));
    window.open_dialog(cx, move |dialog, window, cx| {
        let current = prompt(cx);
        let asked = shown.borrow_mut().update(current.clone()).cloned();
        let (title, description) = asked
            .map(|asked| (asked.title(), asked.describe()))
            .unwrap_or_default();
        let footer = DialogFooter::new()
            .gap_2()
            .child(
                Button::new("discard-cancel")
                    .ghost()
                    .label("Cancel")
                    .on_click({
                        let on_cancel = Rc::clone(&on_cancel);
                        move |_, window, cx| {
                            window.close_dialog(cx);
                            on_cancel(window, cx);
                        }
                    }),
            )
            .child(
                Button::new("discard-confirm")
                    .primary()
                    .label("Discard")
                    .on_click({
                        let on_discard = Rc::clone(&on_discard);
                        let prompt = Rc::clone(&prompt);
                        move |_, window, cx| {
                            let current = prompt(cx);
                            window.close_dialog(cx);
                            if let Some(current) = current {
                                on_discard(current, window, cx);
                            }
                        }
                    }),
            );
        let on_cancel = Rc::clone(&on_cancel);
        dialog
            .w(CONFIRM_WIDTH.to_pixels(window.rem_size()))
            .close_button(false)
            .on_cancel(move |_, window, cx| {
                on_cancel(window, cx);
                true
            })
            .child(
                DialogHeading::new(IconName::Undo2, Tone::Coral, title)
                    .description(div().id("discard-description").child(description)),
            )
            .footer(footer)
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, status: FileStatus, orig_path: Option<&str>) -> ChangedFile {
        ChangedFile {
            path: path.into(),
            status,
            orig_path: orig_path.map(Into::into),
            additions: Some(1),
            deletions: Some(0),
        }
    }

    fn unstaged(file: ChangedFile, also_staged: bool) -> DiscardPrompt {
        DiscardPrompt::File {
            side: DiscardSide::Unstaged,
            file,
            also_staged,
        }
    }

    fn staged(file: ChangedFile) -> DiscardPrompt {
        DiscardPrompt::File {
            side: DiscardSide::Staged,
            file,
            also_staged: false,
        }
    }

    #[test]
    fn names_the_file_by_its_name() {
        let prompt = unstaged(file("src/a.txt", FileStatus::Modified, None), false);
        assert_eq!(prompt.title(), "Discard changes to a.txt?");
        assert_eq!(DiscardPrompt::All.title(), "Discard all changes?");
    }

    #[test]
    fn says_what_discarding_does_to_each_kind_of_file() {
        let modified = file("a.txt", FileStatus::Modified, None);
        assert_eq!(
            unstaged(modified.clone(), false).describe(),
            "The unstaged changes to a.txt are lost."
        );
        assert_eq!(
            unstaged(modified.clone(), true).describe(),
            "The unstaged changes to a.txt are lost. What's staged of it is kept."
        );
        assert_eq!(
            unstaged(file("d.txt", FileStatus::Deleted, None), false).describe(),
            "d.txt is put back as the last commit has it."
        );
        assert_eq!(
            unstaged(file("new.txt", FileStatus::Untracked, None), false).describe(),
            "new.txt isn't tracked, so it's deleted."
        );
        assert_eq!(
            unstaged(file("n.txt", FileStatus::Added, None), false).describe(),
            "n.txt isn't staged yet, so it's deleted."
        );
        assert_eq!(
            unstaged(file("new.txt", FileStatus::Renamed, Some("old.txt")), true).describe(),
            "new.txt is deleted, and old.txt put back as it's staged."
        );
        assert_eq!(
            staged(file("new.txt", FileStatus::Renamed, Some("old.txt"))).describe(),
            "new.txt is deleted, and old.txt put back as the last commit has it: the staged and unstaged changes are lost."
        );
        assert_eq!(
            staged(file("copy.txt", FileStatus::Copied, Some("lib.txt"))).describe(),
            "copy.txt is new, so it's deleted, with its staged and unstaged changes."
        );
        assert_eq!(
            staged(modified).describe(),
            "a.txt is put back as the last commit has it: its staged and unstaged changes are lost."
        );
        assert!(
            DiscardPrompt::All
                .describe()
                .contains("untracked files are deleted")
        );
    }
}
