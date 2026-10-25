//! Asking before deleting a branch. Ported from
//! `components/repository/delete-branch-dialog.tsx`.

use gitto_ui_logic::git::status::branch_name;
use gpui_kit::component::button::ButtonVariant;
use gpui_kit::{App, Window};

use crate::components::ui::{ConfirmDialog, IconName, Tone};

/// A branch, by its short name, with how many commits it has that its remote one doesn't.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unpushed {
    pub name: String,
    pub ahead: u64,
}

/// A branch to delete, and what goes with it, as its menu found them (see `BranchMenu`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchDeletion {
    Local {
        /// A local branch, by its full ref name.
        reference: String,
        /// The remote branch it tracks, by its short name, which is kept; `None` if none.
        upstream: Option<String>,
        /// How many commits it has that its remote branch doesn't.
        ahead: u64,
    },
    Remote {
        /// A remote branch, by its full ref name.
        reference: String,
        /// Whether it's deleted on its remote: not if the remote's gone, its branches left here.
        on_remote: bool,
        /// The local branches tracking it, which are deleted with it.
        tracking: Vec<Unpushed>,
    },
}

impl BranchDeletion {
    /// The branch, by its full ref name.
    pub fn reference(&self) -> &str {
        match self {
            BranchDeletion::Local { reference, .. } | BranchDeletion::Remote { reference, .. } => {
                reference
            }
        }
    }

    /// The dialog's title.
    pub fn title(&self) -> &'static str {
        match self {
            BranchDeletion::Local { .. } => "Delete branch?",
            BranchDeletion::Remote { .. } => "Delete remote branch?",
        }
    }
}

/// `names` in a sentence: `a`, `a and b`, `a, b and c`.
fn listed(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [name] => name.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// `count` commits, in words: "1 commit", "3 commits".
fn commits(count: u64) -> String {
    format!("{count} {}", if count == 1 { "commit" } else { "commits" })
}

/// What deleting `deletion` deletes, and keeps, in words.
pub fn consequences(deletion: &BranchDeletion) -> String {
    let name = branch_name(deletion.reference());
    match deletion {
        BranchDeletion::Local {
            upstream, ahead, ..
        } => {
            let kept = upstream
                .as_ref()
                .map(|upstream| format!(" {upstream} is kept on the remote."))
                .unwrap_or_default();
            let unpushed = match upstream {
                Some(upstream) if *ahead > 0 => {
                    format!(" {name} has {} that {upstream} doesn't.", commits(*ahead))
                }
                _ => String::new(),
            };
            format!("{name} is deleted, with any commits only it has.{kept}{unpushed}")
        }
        BranchDeletion::Remote {
            on_remote,
            tracking,
            ..
        } => {
            let place = if *on_remote {
                format!(
                    "{name} is deleted on the remote, for everyone, with any commits only it has."
                )
            } else {
                format!("{name} is deleted here: its remote is gone.")
            };
            let names: Vec<String> = tracking.iter().map(|local| local.name.clone()).collect();
            let locals = match names.as_slice() {
                [] => String::new(),
                [one] => format!(" So is the local branch {one}, which tracks it."),
                _ => format!(
                    " So are the local branches {}, which track it.",
                    listed(&names)
                ),
            };
            let ahead: Vec<&Unpushed> = tracking.iter().filter(|local| local.ahead > 0).collect();
            let lost = if ahead.len() == 1 && ahead[0].ahead == 1 {
                "which is lost unless another branch has it"
            } else {
                "which are lost unless another branch has them"
            };
            let unpushed = if ahead.is_empty() {
                String::new()
            } else {
                let each: Vec<String> = ahead
                    .iter()
                    .map(|local| format!("{} has {}", local.name, commits(local.ahead)))
                    .collect();
                format!(" {} that {name} doesn't, {lost}.", listed(&each))
            };
            format!("{place}{locals}{unpushed}")
        }
    }
}

/// Asks before deleting a branch, `deletion`, saying what goes with it: for a remote one, the
/// local branches tracking it, and the commits of theirs that were never pushed. Esc, or clicking
/// outside, cancels, which runs `on_cancel`; Delete runs `on_delete`.
pub fn open_delete_branch_dialog(
    deletion: BranchDeletion,
    on_delete: impl Fn(&BranchDeletion, &mut Window, &mut App) + 'static,
    on_cancel: impl Fn(&mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    let description = consequences(&deletion);
    ConfirmDialog::new(IconName::Trash, Tone::Coral, deletion.title())
        .description(description)
        .action("Delete", ButtonVariant::Primary, move |window, cx| {
            on_delete(&deletion, window, cx)
        })
        .on_cancel(on_cancel)
        .open(window, cx);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unpushed(name: &str, ahead: u64) -> Unpushed {
        Unpushed {
            name: name.into(),
            ahead,
        }
    }

    #[test]
    fn says_what_deleting_a_local_branch_keeps() {
        let deletion = BranchDeletion::Local {
            reference: "refs/heads/topic".into(),
            upstream: Some("origin/topic".into()),
            ahead: 2,
        };
        assert_eq!(deletion.title(), "Delete branch?");
        assert_eq!(
            consequences(&deletion),
            "topic is deleted, with any commits only it has. origin/topic is kept on the remote. \
             topic has 2 commits that origin/topic doesn't."
        );
        let alone = BranchDeletion::Local {
            reference: "refs/heads/topic".into(),
            upstream: None,
            ahead: 0,
        };
        assert_eq!(
            consequences(&alone),
            "topic is deleted, with any commits only it has."
        );
    }

    #[test]
    fn says_what_goes_with_a_remote_branch() {
        let deletion = BranchDeletion::Remote {
            reference: "refs/remotes/origin/topic".into(),
            on_remote: true,
            tracking: vec![unpushed("topic", 1)],
        };
        assert_eq!(deletion.title(), "Delete remote branch?");
        assert_eq!(
            consequences(&deletion),
            "origin/topic is deleted on the remote, for everyone, with any commits only it has. \
             So is the local branch topic, which tracks it. topic has 1 commit that origin/topic \
             doesn't, which is lost unless another branch has it."
        );
        let several = BranchDeletion::Remote {
            reference: "refs/remotes/gone/topic".into(),
            on_remote: false,
            tracking: vec![unpushed("a", 2), unpushed("b", 0), unpushed("c", 1)],
        };
        assert_eq!(
            consequences(&several),
            "gone/topic is deleted here: its remote is gone. So are the local branches a, b and \
             c, which track it. a has 2 commits and c has 1 commit that gone/topic doesn't, which \
             are lost unless another branch has them."
        );
    }

    #[test]
    fn lists_names_in_a_sentence() {
        let names = |names: &[&str]| {
            names
                .iter()
                .map(|name| (*name).to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(listed(&names(&[])), "");
        assert_eq!(listed(&names(&["a"])), "a");
        assert_eq!(listed(&names(&["a", "b"])), "a and b");
        assert_eq!(listed(&names(&["a", "b", "c"])), "a, b and c");
    }
}
