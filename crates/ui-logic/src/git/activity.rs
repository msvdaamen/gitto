//! Ported from `git/activity.ts`.

use gitto_git::features::overview::schema::{Activity, ActivityKind};

use super::short_sha;

/// A commit's full SHA, as git names one when there's no branch to name: 40 hex digits, or 64.
fn is_full_sha(target: &str) -> bool {
    matches!(target.len(), 40 | 64)
        && target
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// A branch or commit as git named it, with a commit's SHA shortened.
fn short_name(target: &str) -> &str {
    if is_full_sha(target) {
        short_sha(target)
    } else {
        target
    }
}

/// How to tell something done in a repository (see [`describe_activity`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityDescription {
    /// What to show it as: the commit's subject, for one that made a commit.
    pub title: String,
    /// For one that made a commit, what was done to make it, e.g. "Committed".
    pub action: Option<&'static str>,
}

/// How to tell something done in a repository: what to show it as (the commit's subject, for one
/// that made a commit) and, for those, what was done to make it.
pub fn describe_activity(activity: &Activity) -> ActivityDescription {
    let target = activity
        .target
        .as_deref()
        .filter(|target| !target.is_empty())
        .map(short_name);
    let named = |verb: &str| match target {
        Some(target) => format!("{verb} {target}"),
        None => verb.to_owned(),
    };
    let made = |action: &'static str| ActivityDescription {
        title: activity.subject.clone(),
        action: Some(action),
    };
    let titled = |title: String| ActivityDescription {
        title,
        action: None,
    };
    match activity.kind {
        ActivityKind::Commit => made("Committed"),
        ActivityKind::Amend => made("Amended"),
        ActivityKind::CherryPick => made("Cherry-picked"),
        ActivityKind::Revert => made("Reverted"),
        ActivityKind::Merge => titled(named("Merged")),
        ActivityKind::Pull => titled(named("Pulled")),
        ActivityKind::Rebase => titled(named("Rebased")),
        ActivityKind::Checkout => titled(named("Switched to")),
        ActivityKind::Reset => titled(named("Reset to")),
        ActivityKind::Clone => titled("Cloned".to_owned()),
        // Git's words, with branches by their names, e.g. "Branch: renamed main to trunk".
        ActivityKind::Other => titled(activity.message.replace("refs/heads/", "")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn activity(kind: ActivityKind, target: Option<&str>, message: &str) -> Activity {
        Activity {
            at: 0,
            kind,
            target: target.map(str::to_owned),
            subject: "Fix it".into(),
            message: message.into(),
        }
    }

    fn titled(title: &str) -> ActivityDescription {
        ActivityDescription {
            title: title.into(),
            action: None,
        }
    }

    mod describe_activity {
        use super::*;

        #[test]
        fn shows_a_commit_by_its_subject() {
            assert_eq!(
                describe_activity(&activity(ActivityKind::Commit, None, "")),
                ActivityDescription {
                    title: "Fix it".into(),
                    action: Some("Committed")
                }
            );
            assert_eq!(
                describe_activity(&activity(ActivityKind::Amend, None, "")),
                ActivityDescription {
                    title: "Fix it".into(),
                    action: Some("Amended")
                }
            );
        }

        #[test]
        fn names_what_was_switched_merged_or_reset_to_with_a_commits_sha_shortened() {
            assert_eq!(
                describe_activity(&activity(ActivityKind::Checkout, Some("feature"), "")),
                titled("Switched to feature")
            );
            let sha = "1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b";
            assert_eq!(
                describe_activity(&activity(ActivityKind::Merge, Some(sha), "")),
                titled("Merged 1a2b3c4")
            );
            assert_eq!(
                describe_activity(&activity(ActivityKind::Reset, Some("HEAD~1"), "")),
                titled("Reset to HEAD~1")
            );
            assert_eq!(
                describe_activity(&activity(ActivityKind::Pull, None, "")),
                titled("Pulled")
            );
        }

        #[test]
        fn shows_what_it_doesnt_know_in_gits_words() {
            assert_eq!(
                describe_activity(&activity(
                    ActivityKind::Other,
                    None,
                    "Branch: renamed refs/heads/a to refs/heads/b"
                )),
                titled("Branch: renamed a to b")
            );
        }
    }
}
