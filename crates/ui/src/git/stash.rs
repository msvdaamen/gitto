//! Ported from `git/stash.ts`: whether the changes can be stashed, and a stash popped.
//!
//! TS took `Pick<StatusSummary, "head" | "counts">`; these take those fields of the
//! `StatusSummary`.

use gitto_git::features::stash::schema::Stash;
use gitto_git::features::status::schema::{Head, StatusCounts};

/// Why the changes can't be stashed, if they can't.
pub fn stash_blocker(head: &Head, counts: &StatusCounts) -> Option<&'static str> {
    // Git keeps a stash as commits on top of HEAD, so it needs one.
    if matches!(head, Head::Unborn { .. }) {
        return Some("Make the first commit before stashing changes.");
    }
    if counts.conflicted > 0 {
        return Some("Resolve the conflicts before stashing.");
    }
    if counts.files == 0 {
        return Some("No changes to stash.");
    }
    None
}

/// What stashing would do, e.g. "Stash 3 changed files", or why it can't.
pub fn stash_title(head: &Head, counts: &StatusCounts) -> String {
    match stash_blocker(head, counts) {
        Some(blocker) => blocker.to_owned(),
        None => {
            let files = counts.files;
            format!(
                "Stash {files} changed {}",
                if files == 1 { "file" } else { "files" }
            )
        }
    }
}

/// Why the newest stash can't be popped, if it can't.
pub fn pop_blocker(counts: &StatusCounts, stashes: &[Stash]) -> Option<&'static str> {
    if stashes.is_empty() {
        return Some("No stashes to pop.");
    }
    // Git won't pop over them.
    if counts.conflicted > 0 {
        return Some("Resolve the conflicts before popping a stash.");
    }
    None
}

/// Whether a stash can be popped: the status and the stashes have loaded, and nothing blocks it.
pub fn can_pop(counts: Option<&StatusCounts>, stashes: Option<&[Stash]>) -> bool {
    match (counts, stashes) {
        (Some(counts), Some(stashes)) => pop_blocker(counts, stashes).is_none(),
        _ => false,
    }
}

/// Which stash popping would put back, or why it can't.
pub fn pop_title(counts: &StatusCounts, stashes: &[Stash]) -> String {
    match pop_blocker(counts, stashes) {
        Some(blocker) => blocker.to_owned(),
        None => format!("Pop \"{}\"", stashes[0].message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn head() -> Head {
        Head::Branch {
            name: "main".into(),
            sha: "abc".into(),
        }
    }

    fn counts(files: u64, staged: u64, unstaged: u64, conflicted: u64) -> StatusCounts {
        StatusCounts {
            files,
            staged,
            unstaged,
            conflicted,
        }
    }

    fn changed() -> StatusCounts {
        counts(3, 1, 2, 0)
    }

    fn clean() -> StatusCounts {
        counts(0, 0, 0, 0)
    }

    fn conflicted() -> StatusCounts {
        counts(1, 0, 0, 1)
    }

    fn stash(message: &str) -> Stash {
        Stash {
            sha: "def".into(),
            base: "abc".into(),
            message: message.into(),
            created_at: 0,
        }
    }

    mod stash_title {
        use super::*;

        #[test]
        fn says_how_many_files_would_be_stashed_or_why_they_cant_be() {
            assert_eq!(stash_title(&head(), &changed()), "Stash 3 changed files");
            assert_eq!(
                stash_title(&head(), &counts(1, 0, 1, 0)),
                "Stash 1 changed file"
            );
            assert_eq!(stash_title(&head(), &clean()), "No changes to stash.");
            assert_eq!(
                stash_title(&head(), &conflicted()),
                "Resolve the conflicts before stashing."
            );
            assert_eq!(
                stash_title(
                    &Head::Unborn {
                        name: "main".into()
                    },
                    &changed()
                ),
                "Make the first commit before stashing changes."
            );
        }
    }

    mod pop_title {
        use super::*;

        #[test]
        fn names_the_stash_that_would_be_popped_or_says_why_none_can_be() {
            let first = stash("WIP on main: abc first");
            assert_eq!(
                pop_title(&changed(), &[first.clone(), stash("older")]),
                "Pop \"WIP on main: abc first\""
            );
            // Local changes don't stop a pop: git refuses only if the stash would overwrite them.
            assert_eq!(
                pop_title(&clean(), std::slice::from_ref(&first)),
                "Pop \"WIP on main: abc first\""
            );
            assert_eq!(pop_title(&changed(), &[]), "No stashes to pop.");
            assert_eq!(
                pop_title(&conflicted(), &[first]),
                "Resolve the conflicts before popping a stash."
            );
        }
    }

    #[test]
    fn can_pop_once_the_status_and_stashes_have_loaded() {
        let stashes = [stash("WIP")];
        assert!(can_pop(Some(&clean()), Some(&stashes)));
        assert!(!can_pop(None, Some(&stashes)));
        assert!(!can_pop(Some(&clean()), None));
        assert!(!can_pop(Some(&conflicted()), Some(&stashes)));
    }
}
