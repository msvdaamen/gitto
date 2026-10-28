//! Ported from `git/push.ts`.

use gitto_git::features::remote::schema::PushTarget;

/// What pushing would do, e.g. "Push 2 commits to origin/main", or why it can't.
pub fn push_title(target: &PushTarget) -> String {
    match target {
        PushTarget::Blocked { reason } => reason.to_string(),
        PushTarget::Remote { name, is_new, .. } if *is_new => {
            format!("Push to {name}, a new remote branch")
        }
        PushTarget::Remote { name, ahead: 0, .. } => format!("Push to {name}"),
        PushTarget::Remote { name, ahead, .. } => {
            let commits = if *ahead == 1 { "commit" } else { "commits" };
            format!("Push {ahead} {commits} to {name}")
        }
    }
}

#[cfg(test)]
mod tests {
    use gitto_git::PushBlocker;

    use super::*;

    fn remote(is_new: bool, ahead: u64) -> PushTarget {
        PushTarget::Remote {
            name: "origin/main".into(),
            is_new,
            ahead,
            pulls_from: true,
        }
    }

    #[test]
    fn says_what_pushing_would_do_or_why_it_cant() {
        assert_eq!(
            push_title(&remote(false, 2)),
            "Push 2 commits to origin/main"
        );
        assert_eq!(
            push_title(&remote(false, 1)),
            "Push 1 commit to origin/main"
        );
        assert_eq!(push_title(&remote(false, 0)), "Push to origin/main");
        assert_eq!(
            push_title(&remote(true, 3)),
            "Push to origin/main, a new remote branch"
        );
        assert_eq!(
            push_title(&PushTarget::Blocked {
                reason: PushBlocker::NoRemote
            }),
            "There's no remote to push to."
        );
    }
}
