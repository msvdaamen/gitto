//! Ported from `git/worktree.ts`: where branches are checked out, and new worktrees go.

use gitto_git::features::branch::tracking::{local_name, pick_tracking};
use gitto_git::features::refs::schema::{Ref, RefKind};
use gitto_git::features::worktree::schema::Worktree;

use super::short_sha;
use super::status::branch_name;

/// The worktree the local branch `ref` (a full ref name) is checked out in, if any.
pub fn worktree_on<'a>(worktrees: &'a [Worktree], r#ref: &str) -> Option<&'a Worktree> {
    worktrees
        .iter()
        .find(|worktree| worktree.branch.as_deref() == Some(r#ref))
}

/// The local branches, among `refs`, tracking the remote branch `ref` (a full ref name).
pub fn tracking_branches<'a>(refs: &'a [Ref], r#ref: &str) -> Vec<&'a Ref> {
    let name = branch_name(r#ref);
    refs.iter()
        .filter(|candidate| {
            candidate.kind == RefKind::Local && candidate.upstream.as_deref() == Some(name)
        })
        .collect()
}

/// The local branch switching to the remote branch `ref` (a full ref name), on one of `remotes`,
/// switches to, among `refs`: the one tracking it, the current one or the one named after it if
/// several do, as gitto-git's [`pick_tracking`] picks it; `None` if none does, when a new one is
/// made.
pub fn tracking_branch<'a, S: AsRef<str>>(
    refs: &'a [Ref],
    remotes: &[S],
    r#ref: &str,
) -> Option<&'a Ref> {
    let local = local_name(remotes, branch_name(r#ref));
    let tracking = tracking_branches(refs, r#ref);
    pick_tracking(
        &tracking,
        local,
        |branch| &branch.name,
        |branch| branch.current,
    )
    .copied()
}

/// The worktree checking the branch `ref` (a full ref name) out in a new one would clash with, if
/// any: the one it's checked out in, or for a remote branch, the one the local branch tracking it
/// is (see [`tracking_branch`]), on one of `remotes`.
pub fn checked_out_in<'a, S: AsRef<str>>(
    worktrees: &'a [Worktree],
    refs: &[Ref],
    remotes: &[S],
    r#ref: &str,
) -> Option<&'a Worktree> {
    if !r#ref.starts_with("refs/remotes/") {
        return worktree_on(worktrees, r#ref);
    }
    let local = tracking_branch(refs, remotes, r#ref)?;
    worktree_on(worktrees, &local.full_name)
}

/// Whether a worktree can be opened: not the one on show, a bare one, which has no files, nor one
/// whose folder is gone.
pub fn can_open_worktree(worktree: &Worktree) -> bool {
    !worktree.current && !worktree.bare && worktree.prunable.is_none()
}

/// The path separator `path` uses: `\` on Windows, `/` elsewhere.
fn separator_of(path: &str) -> char {
    if path.contains('\\') && !path.contains('/') {
        '\\'
    } else {
        '/'
    }
}

/// Where to suggest a worktree for the branch `branch` of the repository at `repository`: beside
/// it, named after both, e.g. `/home/me/gitto-feature-login` for `feature/login` of
/// `/home/me/gitto`. The branch's slashes become dashes, so it's one folder.
pub fn suggest_worktree_folder(repository: &str, branch: &str) -> String {
    let trimmed = repository
        .strip_suffix(separator_of(repository))
        .unwrap_or(repository);
    // What a folder's name can't have, in runs, as one dash: `[\\/:*?"<>|\s]+`.
    let mut slug = String::with_capacity(branch.len());
    let mut in_run = false;
    for c in branch.chars() {
        if matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_whitespace() {
            if !in_run {
                slug.push('-');
            }
            in_run = true;
        } else {
            slug.push(c);
            in_run = false;
        }
    }
    let slug = slug.trim_matches('-');
    let slug = if slug.is_empty() { "worktree" } else { slug };
    format!("{trimmed}-{slug}")
}

/// The worktree in words, for a tooltip: its path, the branch it's on or the commit it's at, and
/// whether it's the one on show, locked, or its folder is gone.
pub fn describe_worktree(worktree: &Worktree) -> String {
    let mut lines = vec![worktree.path.clone()];
    if worktree.bare {
        lines.push("Bare: no files are checked out in it".to_owned());
    } else if let Some(branch) = worktree.branch.as_deref().filter(|b| !b.is_empty()) {
        lines.push(format!("On {}", branch_name(branch)));
    } else if let Some(head) = worktree.head.as_deref().filter(|h| !h.is_empty()) {
        lines.push(format!("Detached at {}", short_sha(head)));
    }
    if worktree.current {
        lines.push("Open here".to_owned());
    }
    match worktree.locked.as_deref() {
        Some("") => lines.push("Locked".to_owned()),
        Some(reason) => lines.push(format!("Locked: {reason}")),
        None => {}
    }
    if worktree.prunable.is_some() {
        lines.push("Its folder is gone".to_owned());
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(name: &str, upstream: Option<&str>, current: bool) -> Ref {
        Ref {
            name: name.into(),
            full_name: format!("refs/heads/{name}"),
            kind: RefKind::Local,
            sha: "abc".into(),
            current,
            upstream: upstream.map(str::to_owned),
            ahead: 0,
            behind: 0,
        }
    }

    fn worktree(path: &str, branch: Option<&str>) -> Worktree {
        Worktree {
            path: path.into(),
            name: path[path.rfind('/').map_or(0, |slash| slash + 1)..].to_owned(),
            head: Some("0123456789abcdef".into()),
            branch: branch.map(str::to_owned),
            main: false,
            bare: false,
            current: false,
            locked: None,
            prunable: None,
        }
    }

    fn path(worktree: Option<&Worktree>) -> Option<&str> {
        worktree.map(|worktree| worktree.path.as_str())
    }

    mod worktree_on {
        use super::*;

        #[test]
        fn finds_the_worktree_a_branch_is_checked_out_in_by_its_full_ref_name() {
            let worktrees = [
                worktree("/repo", Some("refs/heads/main")),
                worktree("/repo-f", Some("refs/heads/f")),
            ];
            assert_eq!(
                path(worktree_on(&worktrees, "refs/heads/f")),
                Some("/repo-f")
            );
            assert_eq!(worktree_on(&worktrees, "refs/heads/gone"), None);
            assert_eq!(worktree_on(&[], "refs/heads/main"), None);
        }
    }

    mod tracking_branch {
        use super::*;

        #[test]
        fn picks_the_current_branch_tracking_the_remote_one_else_the_one_named_after_it_else_any() {
            let named = r("feature", Some("origin/feature"), false);
            let other = r("my-feature", Some("origin/feature"), false);
            let current = r("wip", Some("origin/feature"), true);
            let remote = "refs/remotes/origin/feature";
            let tracking = |refs: &[Ref]| tracking_branch(refs, &["origin"], remote).cloned();
            assert_eq!(
                tracking(&[other.clone(), named.clone(), current.clone()]),
                Some(current)
            );
            assert_eq!(tracking(&[other.clone(), named.clone()]), Some(named));
            assert_eq!(tracking(std::slice::from_ref(&other)), Some(other));
            assert_eq!(tracking(&[r("main", Some("origin/main"), false)]), None);
        }

        #[test]
        fn names_the_branch_after_the_remote_one_without_its_whole_remote() {
            // `stream/main` ends the name of `main` on remote `up/stream`, but isn't named after it.
            let stream = r("stream/main", Some("up/stream/main"), false);
            let main = r("main", Some("up/stream/main"), false);
            assert_eq!(
                tracking_branch(
                    &[stream, main.clone()],
                    &["up", "up/stream"],
                    "refs/remotes/up/stream/main"
                ),
                Some(&main)
            );
        }
    }

    mod checked_out_in {
        use super::*;

        fn worktrees() -> Vec<Worktree> {
            vec![
                Worktree {
                    current: true,
                    ..worktree("/repo", Some("refs/heads/main"))
                },
                worktree("/repo-f", Some("refs/heads/feature")),
            ]
        }

        fn refs() -> Vec<Ref> {
            vec![
                r("main", Some("origin/main"), true),
                r("feature", Some("origin/feature"), false),
            ]
        }

        #[test]
        fn is_the_worktree_a_local_branch_is_checked_out_in() {
            let (worktrees, refs) = (worktrees(), refs());
            assert_eq!(
                path(checked_out_in(
                    &worktrees,
                    &refs,
                    &["origin"],
                    "refs/heads/feature"
                )),
                Some("/repo-f")
            );
            assert_eq!(
                checked_out_in(&worktrees, &refs, &["origin"], "refs/heads/other"),
                None
            );
        }

        #[test]
        fn is_the_worktree_the_branch_tracking_a_remote_one_is_checked_out_in() {
            let (worktrees, refs) = (worktrees(), refs());
            let checked_out = |r#ref| path(checked_out_in(&worktrees, &refs, &["origin"], r#ref));
            assert_eq!(checked_out("refs/remotes/origin/main"), Some("/repo"));
            assert_eq!(checked_out("refs/remotes/origin/feature"), Some("/repo-f"));
            assert_eq!(checked_out("refs/remotes/origin/other"), None);
            assert_eq!(
                checked_out_in(&worktrees, &[], &["origin"], "refs/remotes/origin/main"),
                None
            );
        }
    }

    mod can_open_worktree {
        use super::*;

        #[test]
        fn is_false_for_the_one_on_show_a_bare_one_and_one_whose_folder_is_gone() {
            assert!(can_open_worktree(&worktree(
                "/repo-f",
                Some("refs/heads/f")
            )));
            assert!(!can_open_worktree(&Worktree {
                current: true,
                ..worktree("/repo", Some("refs/heads/main"))
            }));
            assert!(!can_open_worktree(&Worktree {
                bare: true,
                head: None,
                ..worktree("/bare.git", None)
            }));
            assert!(!can_open_worktree(&Worktree {
                prunable: Some("gitdir file points elsewhere".into()),
                ..worktree("/gone", None)
            }));
        }
    }

    mod suggest_worktree_folder {
        use super::*;

        #[test]
        fn puts_the_worktree_beside_the_repository_named_after_it_and_the_branch() {
            assert_eq!(
                suggest_worktree_folder("/home/me/gitto", "feature"),
                "/home/me/gitto-feature"
            );
            assert_eq!(
                suggest_worktree_folder("/home/me/gitto/", "feature/login"),
                "/home/me/gitto-feature-login"
            );
            assert_eq!(
                suggest_worktree_folder(r"C:\Users\me\gitto", r"fix\it"),
                r"C:\Users\me\gitto-fix-it"
            );
        }

        #[test]
        fn drops_what_a_folder_name_cant_have_and_never_ends_up_without_a_name() {
            assert_eq!(suggest_worktree_folder("/r", r#"a: "b"?"#), "/r-a-b");
            assert_eq!(suggest_worktree_folder("/r", "///"), "/r-worktree");
        }
    }

    mod describe_worktree {
        use super::*;

        #[test]
        fn says_where_it_is_what_its_on_and_what_state_its_in() {
            assert_eq!(
                describe_worktree(&Worktree {
                    current: true,
                    ..worktree("/repo-f", Some("refs/heads/f"))
                }),
                "/repo-f\nOn f\nOpen here"
            );
            assert_eq!(
                describe_worktree(&Worktree {
                    locked: Some("busy".into()),
                    prunable: Some("gitdir file points elsewhere".into()),
                    ..worktree("/repo-c", None)
                }),
                "/repo-c\nDetached at 0123456\nLocked: busy\nIts folder is gone"
            );
            assert_eq!(
                describe_worktree(&Worktree {
                    locked: Some(String::new()),
                    ..worktree("/repo-l", Some("refs/heads/l"))
                }),
                "/repo-l\nOn l\nLocked"
            );
            assert_eq!(
                describe_worktree(&Worktree {
                    bare: true,
                    head: None,
                    ..worktree("/bare.git", None)
                }),
                "/bare.git\nBare: no files are checked out in it"
            );
        }
    }
}
