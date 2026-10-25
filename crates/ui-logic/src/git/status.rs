//! Ported from `git/status.ts`: what to call HEAD, and how it compares to its upstream.
//!
//! TS took `Pick`s of the `StatusSummary`; these take those of its fields.

use gitto_git::features::remote::blockers::pull_blocker;
use gitto_git::features::status::schema::{Head, StatusSummary};

/// What to call HEAD: the branch name, or "detached HEAD".
pub fn head_label(head: &Head) -> &str {
    head_branch(head).unwrap_or("detached HEAD")
}

/// The commit HEAD points at; `None` on a branch without commits yet.
pub fn head_sha(head: &Head) -> Option<&str> {
    match head {
        Head::Unborn { .. } => None,
        Head::Branch { sha, .. } | Head::Detached { sha } => Some(sha),
    }
}

/// HEAD's branch; `None` when it's detached.
fn head_branch(head: &Head) -> Option<&str> {
    match head {
        Head::Detached { .. } => None,
        Head::Branch { name, .. } | Head::Unborn { name } => Some(name),
    }
}

/// A branch's short name, from its full ref name: `main`, `origin/feature`.
pub fn branch_name(r#ref: &str) -> &str {
    r#ref
        .strip_prefix("refs/heads/")
        .or_else(|| r#ref.strip_prefix("refs/remotes/"))
        .unwrap_or(r#ref)
}

/// Whether HEAD is on the branch `ref`, by its full ref name, e.g. `refs/heads/main`.
pub fn is_checked_out(head: &Head, r#ref: &str) -> bool {
    head_branch(head).is_some_and(|name| r#ref.strip_prefix("refs/heads/") == Some(name))
}

pub fn has_uncommitted_changes(status: Option<&StatusSummary>) -> bool {
    status.is_some_and(|status| status.counts.files > 0)
}

/// How HEAD compares to its upstream: "synced", or how many commits ahead and behind, e.g. "↑2 ↓1".
pub fn sync_label(ahead: u64, behind: u64) -> String {
    let parts: Vec<String> = [(ahead, '↑'), (behind, '↓')]
        .into_iter()
        .filter(|&(count, _)| count > 0)
        .map(|(count, arrow)| format!("{arrow}{count}"))
        .collect();
    if parts.is_empty() {
        "synced".to_owned()
    } else {
        parts.join(" ")
    }
}

/// Why HEAD can't be pulled into, if it can't: it isn't on a branch, or the branch has no upstream.
pub fn head_pull_blocker(head: &Head, upstream: Option<&str>) -> Option<String> {
    pull_blocker(head_branch(head), upstream)
}

/// What pulling would do, e.g. "Pull 2 commits from origin/main", or why it can't.
pub fn pull_title(head: &Head, upstream: Option<&str>, behind: u64) -> String {
    if let Some(blocker) = head_pull_blocker(head, upstream) {
        return blocker;
    }
    let upstream = upstream.unwrap_or_default();
    // `behind` is as of the last fetch; the pull fetches first, so there may be more.
    match behind {
        0 => format!("Pull from {upstream}"),
        1 => format!("Pull 1 commit from {upstream}"),
        _ => format!("Pull {behind} commits from {upstream}"),
    }
}

/// What a new branch is made from: the branch's name, or "this commit" with HEAD detached.
pub fn branch_source(head: &Head) -> &str {
    head_branch(head).unwrap_or("this commit")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn main() -> Head {
        Head::Branch {
            name: "main".into(),
            sha: "abc".into(),
        }
    }

    mod sync_label {
        use super::*;

        #[test]
        fn says_synced_or_how_far_ahead_and_behind_head_is() {
            assert_eq!(sync_label(0, 0), "synced");
            assert_eq!(sync_label(2, 0), "↑2");
            assert_eq!(sync_label(2, 1), "↑2 ↓1");
        }
    }

    mod pull_title {
        use super::*;

        const TRACKING: Option<&str> = Some("origin/main");

        #[test]
        fn says_how_many_commits_there_are_to_pull_as_of_the_last_fetch() {
            assert_eq!(pull_title(&main(), TRACKING, 0), "Pull from origin/main");
            assert_eq!(
                pull_title(&main(), TRACKING, 1),
                "Pull 1 commit from origin/main"
            );
            assert_eq!(
                pull_title(&main(), TRACKING, 3),
                "Pull 3 commits from origin/main"
            );
        }

        #[test]
        fn says_why_theres_nothing_to_pull() {
            assert_eq!(
                pull_title(&main(), None, 0),
                "main doesn't track a remote branch."
            );
            assert_eq!(
                pull_title(&Head::Detached { sha: "abc".into() }, TRACKING, 0),
                "Not on a branch: HEAD is detached, or a rebase is under way."
            );
        }
    }

    #[test]
    fn names_head_and_its_branch() {
        let detached = Head::Detached { sha: "abc".into() };
        assert_eq!(head_label(&main()), "main");
        assert_eq!(head_label(&detached), "detached HEAD");
        assert_eq!(
            head_sha(&Head::Unborn {
                name: "main".into()
            }),
            None
        );
        assert_eq!(branch_name("refs/remotes/origin/feature"), "origin/feature");
        assert!(is_checked_out(&main(), "refs/heads/main"));
        assert!(!is_checked_out(&detached, "refs/heads/main"));
        assert_eq!(branch_source(&detached), "this commit");
    }
}
