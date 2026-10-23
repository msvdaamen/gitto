//! Shared with the UI, which names a remote branch's remote the same way, so this uses nothing
//! else of the crate.

/// The remote, among `remotes`, the remote branch `name` (e.g. `origin/feature`) is on: the
/// longest one it starts with, as a remote's name can have a slash in it. `None` when it's gone,
/// its branches left.
pub fn remote_of<'a, S: AsRef<str>>(remotes: &'a [S], name: &str) -> Option<&'a str> {
    remotes
        .iter()
        .map(AsRef::as_ref)
        .filter(|remote| {
            !remote.is_empty()
                && name
                    .strip_prefix(remote)
                    .is_some_and(|rest| rest.starts_with('/'))
        })
        .fold(None, |longest: Option<&str>, remote| {
            if remote.len() > longest.map_or(0, str::len) {
                Some(remote)
            } else {
                longest
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_the_longest_remote_its_name_starts_with() {
        assert_eq!(
            remote_of(&["origin", "team", "team/origin"], "team/origin/main"),
            Some("team/origin")
        );
        assert_eq!(remote_of(&["origin"], "origin/feature/x"), Some("origin"));
    }

    #[test]
    fn is_none_for_a_branch_of_a_remote_thats_gone() {
        assert_eq!(remote_of(&["origin"], "old/main"), None);
        assert_eq!(remote_of(&["orig"], "origin/main"), None);
    }
}
