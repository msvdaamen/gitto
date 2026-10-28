//! Which local branch switching to a remote branch switches to. Shared with the UI, which warns
//! about the worktree it's checked out in, so this uses nothing else of the crate but
//! [`remote_of`].

use crate::features::remote::remote_name::remote_of;

/// The name of the local branch for the remote branch `name` (e.g. `origin/feature`) among
/// `remotes`: its name without its remote (`feature`), or, for a remote that's gone, its branches
/// left, without the name's first part.
pub fn local_name<'a, S: AsRef<str>>(remotes: &[S], name: &'a str) -> &'a str {
    match remote_of(remotes, name) {
        Some(remote) => &name[remote.len() + 1..],
        None => name.split_once('/').map_or(name, |(_, local)| local),
    }
}

/// Which of the local branches tracking a remote branch, `tracking`, switching to it switches to:
/// the current one, else the one named `local` (see [`local_name`]), else the first; `None` when
/// none tracks it, when a new one is made. `name` and `current` tell a branch's short name and
/// whether it's checked out here.
pub fn pick_tracking<'a, T>(
    tracking: &'a [T],
    local: &str,
    name: impl Fn(&T) -> &str,
    current: impl Fn(&T) -> bool,
) -> Option<&'a T> {
    tracking
        .iter()
        .find(|&branch| current(branch))
        .or_else(|| tracking.iter().find(|&branch| name(branch) == local))
        .or_else(|| tracking.first())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_the_local_branch_after_the_remote_branch_without_its_remote() {
        assert_eq!(
            local_name(&["origin"], "origin/feature/login"),
            "feature/login"
        );
        assert_eq!(local_name(&["up", "up/stream"], "up/stream/main"), "main");
        assert_eq!(local_name(&["origin"], "old/main"), "main");
        assert_eq!(local_name(&["origin"], "backup"), "backup");
    }

    #[test]
    fn picks_the_current_branch_else_the_one_named_after_the_remote_one_else_the_first() {
        let pick = |tracking: &[(&'static str, bool)]| {
            pick_tracking(tracking, "feature", |branch| branch.0, |branch| branch.1)
                .map(|branch| branch.0)
        };
        let (other, named, current) = (("mine", false), ("feature", false), ("wip", true));
        assert_eq!(pick(&[other, named, current]), Some("wip"));
        assert_eq!(pick(&[other, named]), Some("feature"));
        assert_eq!(pick(&[other]), Some("mine"));
        assert_eq!(pick(&[]), None);
    }
}
