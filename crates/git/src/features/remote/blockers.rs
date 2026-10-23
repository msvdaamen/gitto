//! Why a pull or push can't start. Shared with the UI, which disables pulling for the same
//! reasons, so this uses nothing else of the crate.

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
pub fn detached_head(action: Action, rebasing: bool) -> String {
    if rebasing {
        return format!(
            "A rebase is under way. Continue or abort it, then {}.",
            action.as_str()
        );
    }
    let what = match action {
        Action::Pull => "pull into",
        Action::Push => "push",
    };
    format!("HEAD is detached: check out a branch to {what} it.")
}

/// Why a detached HEAD can't be pulled into, when no rebase is under way.
pub fn no_branch() -> String {
    detached_head(Action::Pull, false)
}

/// Why HEAD can't be pulled into while a rebase is stopped, which detaches it.
pub fn rebasing() -> String {
    detached_head(Action::Pull, true)
}

/// Why a detached HEAD can't be pulled into, when it's not known whether a rebase is under way:
/// the status, which the UI goes by, doesn't say.
pub const DETACHED: &str = "Not on a branch: HEAD is detached, or a rebase is under way.";

/// Why `branch` can't be pulled into when it has no upstream.
pub fn no_upstream(branch: &str) -> String {
    format!("{branch} doesn't track a remote branch.")
}

/// Why `branch` (`None` when HEAD is detached) can't be pulled into, if it can't, going by what
/// the status says; it can't tell a detached HEAD from a rebase under way.
pub fn pull_blocker(branch: Option<&str>, upstream: Option<&str>) -> Option<String> {
    let Some(branch) = branch else {
        return Some(DETACHED.to_owned());
    };
    if upstream.is_none_or(str::is_empty) {
        return Some(no_upstream(branch));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pull_blocker_is_why_a_branch_cant_be_pulled_into_going_by_the_status() {
        assert_eq!(pull_blocker(Some("main"), Some("origin/main")), None);
        assert_eq!(pull_blocker(Some("main"), None), Some(no_upstream("main")));
        assert_eq!(
            pull_blocker(Some("main"), Some("")),
            Some(no_upstream("main"))
        );
    }

    #[test]
    fn pull_blocker_cant_tell_a_detached_head_from_a_rebase_under_way() {
        assert!(
            pull_blocker(None, None)
                .unwrap()
                .contains("detached, or a rebase")
        );
        assert!(
            pull_blocker(None, Some("origin/main"))
                .unwrap()
                .contains("detached, or a rebase")
        );
    }
}
