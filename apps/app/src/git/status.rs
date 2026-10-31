//! From `git/status.ts` (see the module's doc).

use gitto_git::features::status::schema::{Head, StatusSummary};

/// What to call HEAD: the branch name, or "detached HEAD".
pub fn head_label(head: &Head) -> &str {
    match head {
        Head::Detached { .. } => "detached HEAD",
        Head::Branch { name, .. } | Head::Unborn { name } => name,
    }
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
    }
}
