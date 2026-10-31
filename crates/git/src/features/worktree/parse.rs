//! Reading `git worktree list`. Ported from `parse.ts`.

use super::schema::{Lock, Worktree};

/// How `worktree list --porcelain -z` is read: every line ends in a NUL, and an empty line ends
/// each worktree.
pub const WORKTREE_LIST_ARGS: [&str; 4] = ["worktree", "list", "--porcelain", "-z"];

/// `path`, which git writes with `/`, as the OS writes it: `C:\…` on Windows.
fn native(path: &str) -> String {
    if cfg!(windows) {
        path.replace('/', "\\")
    } else {
        path.to_owned()
    }
}

/// The last part of `path`, as Node's `basename` has it: trailing slashes left out.
fn basename(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(slash) => &trimmed[slash + 1..],
        None => trimmed,
    }
}

/// The worktrees in `worktree list --porcelain -z`'s output (see `WORKTREE_LIST_ARGS`), the main
/// one first; none is `current` yet (see `list_worktrees`).
pub fn parse_worktree_list(output: &str) -> Vec<Worktree> {
    let mut worktrees: Vec<Worktree> = Vec::new();
    for entry in output.split("\0\0") {
        let lines: Vec<&str> = entry.split('\0').filter(|line| !line.is_empty()).collect();
        let Some(path) = lines.iter().find_map(|line| line.strip_prefix("worktree ")) else {
            continue;
        };
        let attribute = |name: &str| -> Option<String> {
            lines.iter().find_map(|line| {
                let rest = line.strip_prefix(name)?;
                if rest.is_empty() {
                    Some(String::new())
                } else {
                    rest.strip_prefix(' ').map(str::to_owned)
                }
            })
        };
        worktrees.push(Worktree {
            path: native(path),
            name: basename(path).to_owned(),
            head: attribute("HEAD"),
            branch: attribute("branch"),
            main: worktrees.is_empty(),
            bare: attribute("bare").is_some(),
            current: false,
            locked: attribute("locked").map(|reason| Lock {
                reason: (!reason.is_empty()).then_some(reason),
            }),
            prunable: attribute("prunable"),
        });
    }
    worktrees
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `worktree list --porcelain -z`'s output for the worktrees `entries`, each its lines.
    fn output(entries: &[&[&str]]) -> String {
        entries
            .iter()
            .map(|lines| format!("{}\0\0", lines.join("\0")))
            .collect()
    }

    #[test]
    fn reads_the_main_worktree_first_then_the_linked_ones_on_a_branch_or_detached() {
        let worktrees = parse_worktree_list(&output(&[
            &["worktree /repo", "HEAD aaa", "branch refs/heads/main"],
            &[
                "worktree /repo-feature",
                "HEAD bbb",
                "branch refs/heads/feature",
            ],
            &["worktree /tmp/repo-check", "HEAD ccc", "detached"],
        ]));
        assert_eq!(worktrees.len(), 3);
        assert_eq!(
            worktrees[0],
            Worktree {
                path: native("/repo"),
                name: "repo".to_owned(),
                head: Some("aaa".to_owned()),
                branch: Some("refs/heads/main".to_owned()),
                main: true,
                bare: false,
                current: false,
                locked: None,
                prunable: None,
            }
        );
        let feature = &worktrees[1];
        assert_eq!(feature.path, "/repo-feature");
        assert_eq!(feature.name, "repo-feature");
        assert_eq!(feature.head.as_deref(), Some("bbb"));
        assert_eq!(feature.branch.as_deref(), Some("refs/heads/feature"));
        assert!(!feature.main);
        let check = &worktrees[2];
        assert_eq!(check.path, "/tmp/repo-check");
        assert_eq!(check.head.as_deref(), Some("ccc"));
        assert_eq!(check.branch, None);
        assert!(!check.main);
    }

    #[test]
    fn reads_why_a_worktree_is_locked_or_prunable_and_a_lock_without_a_reason() {
        let worktrees = parse_worktree_list(&output(&[
            &["worktree /a", "HEAD aaa", "detached", "locked busy now"],
            &["worktree /b", "bare"],
            &[
                "worktree /c",
                "HEAD ccc",
                "branch refs/heads/x",
                "locked",
                "prunable gitdir file points to non-existent location",
            ],
        ]));
        let [locked, bare, prunable] = &worktrees[..] else {
            panic!("{worktrees:?}");
        };
        assert_eq!(
            locked.locked,
            Some(Lock {
                reason: Some("busy now".to_owned())
            })
        );
        assert_eq!(locked.prunable, None);
        assert!(bare.bare);
        assert_eq!(bare.head, None);
        assert_eq!(bare.branch, None);
        assert_eq!(prunable.locked, Some(Lock { reason: None }));
        assert_eq!(
            prunable.prunable.as_deref(),
            Some("gitdir file points to non-existent location")
        );
    }

    #[test]
    fn keeps_a_path_with_a_newline_in_it_whole() {
        let worktrees =
            parse_worktree_list(&output(&[&["worktree /a\nb", "HEAD aaa", "detached"]]));
        assert_eq!(worktrees.len(), 1);
        assert_eq!(worktrees[0].path, "/a\nb");
        assert_eq!(worktrees[0].name, "a\nb");
    }

    #[test]
    fn reads_nothing_from_no_output() {
        assert_eq!(parse_worktree_list(""), Vec::new());
    }
}
