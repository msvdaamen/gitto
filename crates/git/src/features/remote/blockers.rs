//! Why a pull or push can't start, as reasons the UI can match on, each worded as a message by
//! its `Display`. Shared with the UI, which disables pulling for the same reasons, so this reads
//! nothing.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::features::branch::in_use::InUse;

/// What's being done to a branch (see `detached_head`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Pull,
    Push,
}

impl Action {
    pub fn as_str(self) -> &'static str {
        match self {
            Action::Pull => "pull",
            Action::Push => "push",
        }
    }
}

/// Why HEAD can't be pulled into or pushed while it's detached: `rebasing` when that's as a rebase
/// stopped, which is to finish rather than a branch to check out.
fn detached_head(f: &mut fmt::Formatter<'_>, action: Action, rebasing: bool) -> fmt::Result {
    if rebasing {
        return write!(
            f,
            "A rebase is under way. Continue or abort it, then {}.",
            action.as_str()
        );
    }
    let what = match action {
        Action::Pull => "pull into",
        Action::Push => "push",
    };
    write!(f, "HEAD is detached: check out a branch to {what} it.")
}

/// Why the checked-out branch can't be pushed (see `Error::PushBlocked`, and `PushTarget`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PushBlocker {
    /// HEAD isn't on a branch: `rebasing` when that's as a rebase stopped.
    Detached { rebasing: bool },
    /// `branch` has no commits yet.
    Unborn { branch: String },
    /// There's no remote to push to.
    NoRemote,
    /// `remote`'s push refspecs don't push `branch` anywhere.
    NotPushed { remote: String, branch: String },
}

impl fmt::Display for PushBlocker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PushBlocker::Detached { rebasing } => detached_head(f, Action::Push, *rebasing),
            PushBlocker::Unborn { branch } => write!(f, "{branch} has no commits to push yet."),
            PushBlocker::NoRemote => f.write_str("There's no remote to push to."),
            PushBlocker::NotPushed { remote, branch } => {
                write!(f, "{remote}'s push settings don't push {branch} anywhere.")
            }
        }
    }
}

/// Why a branch can't be pulled (see `Error::PullBlocked`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PullBlocker {
    /// HEAD isn't on a branch: `rebasing` when that's as a rebase stopped, which is to finish
    /// rather than a branch to check out; `None` when it isn't known which, as the status, which
    /// the UI goes by, doesn't say.
    Detached { rebasing: Option<bool> },
    /// `branch` doesn't track a remote branch.
    NoUpstream { branch: String },
    /// `branch` tracks several branches, which only `git pull` merges at once.
    SeveralUpstreams { branch: String },
    /// The branch pulled from, `upstream` by its short name, no longer exists: on `remote`, or in
    /// the repository itself (`None`).
    UpstreamGone {
        upstream: String,
        remote: Option<String>,
    },
    /// `branch`, which isn't checked out here, is in use in a worktree, so pulling it would need
    /// it checked out.
    InUse { branch: String, using: InUse },
    /// `branch`, which isn't checked out here, has commits its upstream (`upstream`, e.g.
    /// `origin/main`) doesn't, which only merging or rebasing it, checked out, brings together.
    Diverged { branch: String, upstream: String },
}

impl fmt::Display for PullBlocker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PullBlocker::Detached {
                rebasing: Some(rebasing),
            } => detached_head(f, Action::Pull, *rebasing),
            PullBlocker::Detached { rebasing: None } => {
                f.write_str("Not on a branch: HEAD is detached, or a rebase is under way.")
            }
            PullBlocker::NoUpstream { branch } => {
                write!(f, "{branch} doesn't track a remote branch.")
            }
            PullBlocker::SeveralUpstreams { branch } => write!(
                f,
                "{branch} tracks several branches, which only git pull, in a terminal, merges at once."
            ),
            PullBlocker::UpstreamGone {
                upstream,
                remote: Some(remote),
            } => write!(f, "{upstream} no longer exists on {remote}."),
            PullBlocker::UpstreamGone {
                upstream,
                remote: None,
            } => write!(f, "{upstream} no longer exists."),
            PullBlocker::InUse { branch, using } => write!(
                f,
                "{branch} {}, so it wasn't pulled. {}",
                using.state, using.fix
            ),
            PullBlocker::Diverged { branch, upstream } => write!(
                f,
                "{branch} and {upstream} have diverged. Switch to {branch} to pull it."
            ),
        }
    }
}

/// Why `branch` (`None` when HEAD is detached) can't be pulled into, if it can't, going by what
/// the status says; it can't tell a detached HEAD from a rebase under way.
pub fn pull_blocker(branch: Option<&str>, upstream: Option<&str>) -> Option<PullBlocker> {
    let Some(branch) = branch else {
        return Some(PullBlocker::Detached { rebasing: None });
    };
    if upstream.is_none_or(str::is_empty) {
        return Some(PullBlocker::NoUpstream {
            branch: branch.to_owned(),
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_upstream(branch: &str) -> Option<PullBlocker> {
        Some(PullBlocker::NoUpstream {
            branch: branch.to_owned(),
        })
    }

    #[test]
    fn pull_blocker_is_why_a_branch_cant_be_pulled_into_going_by_the_status() {
        assert_eq!(pull_blocker(Some("main"), Some("origin/main")), None);
        assert_eq!(pull_blocker(Some("main"), None), no_upstream("main"));
        assert_eq!(pull_blocker(Some("main"), Some("")), no_upstream("main"));
    }

    #[test]
    fn pull_blocker_cant_tell_a_detached_head_from_a_rebase_under_way() {
        let detached = Some(PullBlocker::Detached { rebasing: None });
        assert_eq!(pull_blocker(None, None), detached);
        assert_eq!(pull_blocker(None, Some("origin/main")), detached);
        assert_eq!(
            PullBlocker::Detached { rebasing: None }.to_string(),
            "Not on a branch: HEAD is detached, or a rebase is under way."
        );
    }

    #[test]
    fn says_why_a_pull_cant_start() {
        let said = |blocker: PullBlocker| blocker.to_string();
        assert_eq!(
            said(PullBlocker::Detached {
                rebasing: Some(false)
            }),
            "HEAD is detached: check out a branch to pull into it."
        );
        assert_eq!(
            said(PullBlocker::Detached {
                rebasing: Some(true)
            }),
            "A rebase is under way. Continue or abort it, then pull."
        );
        assert_eq!(
            said(PullBlocker::UpstreamGone {
                upstream: "gone".into(),
                remote: Some("origin".into())
            }),
            "gone no longer exists on origin."
        );
        assert_eq!(
            said(PullBlocker::UpstreamGone {
                upstream: "other".into(),
                remote: None
            }),
            "other no longer exists."
        );
        assert_eq!(
            said(PullBlocker::Diverged {
                branch: "main".into(),
                upstream: "origin/main".into()
            }),
            "main and origin/main have diverged. Switch to main to pull it."
        );
    }

    #[test]
    fn says_why_a_push_cant_start() {
        let said = |blocker: PushBlocker| blocker.to_string();
        assert_eq!(
            said(PushBlocker::Detached { rebasing: false }),
            "HEAD is detached: check out a branch to push it."
        );
        assert_eq!(
            said(PushBlocker::Detached { rebasing: true }),
            "A rebase is under way. Continue or abort it, then push."
        );
        assert_eq!(
            said(PushBlocker::Unborn {
                branch: "main".into()
            }),
            "main has no commits to push yet."
        );
        assert_eq!(said(PushBlocker::NoRemote), "There's no remote to push to.");
        assert_eq!(
            said(PushBlocker::NotPushed {
                remote: "origin".into(),
                branch: "main".into()
            }),
            "origin's push settings don't push main anywhere."
        );
    }
}
