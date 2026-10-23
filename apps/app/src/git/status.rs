//! From `git/status.ts` (see the module's doc).

use gitto_git::features::status::schema::{Head, StatusSummary};

/// What to call HEAD: the branch name, or "detached HEAD".
pub fn head_label(head: &Head) -> &str {
    match head {
        Head::Detached { .. } => "detached HEAD",
        Head::Branch { name, .. } | Head::Unborn { name } => name,
    }
}

/// A branch's short name, from its full ref name: `main`, `origin/feature`.
pub fn branch_name(reference: &str) -> &str {
    reference
        .strip_prefix("refs/heads/")
        .or_else(|| reference.strip_prefix("refs/remotes/"))
        .unwrap_or(reference)
}

/// Whether HEAD is on the branch `reference`, by its full ref name, e.g. `refs/heads/main`.
pub fn is_checked_out(head: &Head, reference: &str) -> bool {
    match head {
        Head::Detached { .. } => false,
        Head::Branch { name, .. } | Head::Unborn { name } => {
            reference.strip_prefix("refs/heads/") == Some(name.as_str())
        }
    }
}

pub fn has_uncommitted_changes(status: Option<&StatusSummary>) -> bool {
    status.is_some_and(|status| status.counts.files > 0)
}

/// How HEAD compares to its upstream: "synced", or how many commits ahead and behind, e.g.
/// "↑2 ↓1".
pub fn sync_label(status: &StatusSummary) -> String {
    let mut parts = Vec::new();
    if status.ahead > 0 {
        parts.push(format!("↑{}", status.ahead));
    }
    if status.behind > 0 {
        parts.push(format!("↓{}", status.behind));
    }
    if parts.is_empty() {
        "synced".to_owned()
    } else {
        parts.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use gitto_git::features::status::schema::StatusCounts;

    use super::*;

    #[test]
    fn names_head_and_how_it_compares() {
        assert_eq!(
            head_label(&Head::Detached { sha: "abc".into() }),
            "detached HEAD"
        );
        assert_eq!(
            head_label(&Head::Unborn {
                name: "main".into()
            }),
            "main"
        );
        let mut status = StatusSummary {
            head: Head::Unborn {
                name: "main".into(),
            },
            upstream: Some("origin/main".into()),
            ahead: 0,
            behind: 0,
            counts: StatusCounts {
                files: 0,
                staged: 0,
                unstaged: 0,
                conflicted: 0,
            },
        };
        assert_eq!(sync_label(&status), "synced");
        status.ahead = 2;
        status.behind = 1;
        assert_eq!(sync_label(&status), "↑2 ↓1");
        assert!(!has_uncommitted_changes(Some(&status)));
        assert!(!has_uncommitted_changes(None));
        status.counts.files = 1;
        assert!(has_uncommitted_changes(Some(&status)));
    }

    #[test]
    fn names_branches_and_tells_which_is_checked_out() {
        assert_eq!(branch_name("refs/heads/main"), "main");
        assert_eq!(branch_name("refs/remotes/origin/feature"), "origin/feature");
        let head = Head::Branch {
            name: "main".into(),
            sha: "abc".into(),
        };
        assert!(is_checked_out(&head, "refs/heads/main"));
        assert!(!is_checked_out(&head, "refs/remotes/origin/main"));
        assert!(!is_checked_out(
            &Head::Detached { sha: "abc".into() },
            "refs/heads/main"
        ));
    }
}
