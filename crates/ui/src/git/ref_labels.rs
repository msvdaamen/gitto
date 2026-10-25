//! Ported from `git/ref-labels.ts`: the labels in the history's branch / tag column.

use std::collections::HashMap;

use gitto_git::features::history::schema::{CommitRef, CommitRefKind};
use gitto_git::features::worktree::schema::Worktree;
use indexmap::IndexMap;

/// A branch in the history's branch / tag column: local, on remotes, or both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchLabel {
    /// `main` for a local branch (and its remote copies), `origin/feature` for a remote one.
    pub name: String,
    /// The full ref name to switch to (TS's `ref`): the local branch's, e.g. `refs/heads/main`, or
    /// for a remote one `refs/remotes/origin/feature` (switching to it switches to the local branch
    /// tracking it).
    pub full_name: String,
    /// Whether a local branch points here.
    pub local: bool,
    /// Remotes whose copy of the branch points here, e.g. `origin`.
    pub remotes: Vec<String>,
    /// Whether it's the checked-out branch.
    pub current: bool,
    /// The other worktree it's checked out in, if one is, named after its folder: a branch is checked
    /// out in one worktree at a time.
    pub worktree: Option<WorktreeAt>,
}

/// A worktree, by its folder's name and path.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WorktreeAt {
    pub name: String,
    pub path: String,
}

/// A label in the history's branch / tag column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefLabel {
    Branch(BranchLabel),
    Tag {
        name: String,
    },
    /// A detached HEAD.
    Head,
    /// Another worktree, named after its folder, checked out at the commit with no branch.
    Worktree(WorktreeAt),
}

impl RefLabel {
    /// What the label says: the branch's, tag's or worktree's name, or `HEAD`.
    pub fn name(&self) -> &str {
        match self {
            RefLabel::Branch(branch) => &branch.name,
            RefLabel::Tag { name } => name,
            RefLabel::Head => "HEAD",
            RefLabel::Worktree(worktree) => &worktree.name,
        }
    }
}

/// Where the other worktrees are checked out, for the labels to show. The default has none: TS's
/// `NO_CHECKOUTS`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Checkouts {
    /// Each worktree, by the full ref name of the branch checked out in it.
    pub branches: HashMap<String, WorktreeAt>,
    /// The worktrees checked out at each commit with no branch, by its SHA, in git's order.
    pub detached: IndexMap<String, Vec<WorktreeAt>>,
}

/// Where the worktrees other than the one on show are checked out (see [`Checkouts`]); a bare one
/// isn't checked out anywhere.
pub fn checkouts(worktrees: &[Worktree]) -> Checkouts {
    let mut others = Checkouts::default();
    for worktree in worktrees {
        if worktree.current || worktree.bare {
            continue;
        }
        let at = WorktreeAt {
            name: worktree.name.clone(),
            path: worktree.path.clone(),
        };
        if let Some(branch) = worktree.branch.as_ref().filter(|branch| !branch.is_empty()) {
            others.branches.insert(branch.clone(), at);
        } else if let Some(head) = worktree.head.as_ref().filter(|head| !head.is_empty()) {
            others.detached.entry(head.clone()).or_default().push(at);
        }
    }
    others
}

/// Merges the refs of the commit `sha` into labels, like GitKraken: a local branch and its remote
/// copies (`main`, `origin/main`) share one label. A detached HEAD comes first, then the
/// checked-out branch, other local branches, the other worktrees checked out at the commit with no
/// branch (`others`), remote-only branches and tags. A branch checked out in another worktree
/// says which.
pub fn to_ref_labels(refs: &[CommitRef], sha: &str, others: &Checkouts) -> Vec<RefLabel> {
    let detached = others.detached.get(sha).map_or(&[][..], Vec::as_slice);
    if refs.is_empty() && detached.is_empty() {
        return Vec::new();
    }
    let head = refs.iter().any(|r| r.kind == CommitRefKind::Head);

    // The local branches, in git's order, and where each is by its name.
    let mut locals: Vec<BranchLabel> = Vec::new();
    let mut local_names: HashMap<&str, usize> = HashMap::new();
    for r in refs.iter().filter(|r| r.kind == CommitRefKind::Local) {
        let label = BranchLabel {
            name: r.name.clone(),
            full_name: r.full_name.clone(),
            local: true,
            remotes: Vec::new(),
            current: r.current.unwrap_or(false),
            worktree: others.branches.get(&r.full_name).cloned(),
        };
        // As a `Map` does: a name set again keeps its place.
        match local_names.get(r.name.as_str()) {
            Some(&index) => locals[index] = label,
            None => {
                local_names.insert(&r.name, locals.len());
                locals.push(label);
            }
        }
    }
    // Remote branches without a local copy here; kept apart from the local ones, as a local branch
    // can be named like a remote one (`origin/main`).
    let mut remote_only: Vec<BranchLabel> = Vec::new();

    for r in refs.iter().filter(|r| r.kind == CommitRefKind::Remote) {
        // `origin/feature/login` is branch `feature/login` on remote `origin`.
        let split = r.name.split_once('/');
        let remotes: Vec<String> = split
            .map(|(remote, _)| vec![remote.to_owned()])
            .unwrap_or_default();
        let local = split.and_then(|(_, branch)| local_names.get(branch).copied());
        match local {
            Some(index) => locals[index].remotes.extend(remotes),
            None => remote_only.push(BranchLabel {
                name: r.name.clone(),
                full_name: r.full_name.clone(),
                local: false,
                remotes,
                current: false,
                worktree: None,
            }),
        }
    }

    // The checked-out branch first; otherwise in git's order.
    locals.sort_by_key(|branch| !branch.current);

    let mut labels = Vec::new();
    if head {
        labels.push(RefLabel::Head);
    }
    labels.extend(locals.into_iter().map(RefLabel::Branch));
    labels.extend(detached.iter().cloned().map(RefLabel::Worktree));
    labels.extend(remote_only.into_iter().map(RefLabel::Branch));
    labels.extend(
        refs.iter()
            .filter(|r| r.kind == CommitRefKind::Tag)
            .map(|r| RefLabel::Tag {
                name: r.name.clone(),
            }),
    );
    labels
}

/// Whether a search matches the label: its name, a remote copy's full name (`origin/main`), `HEAD`
/// for the checked-out branch, the worktree a branch is checked out in or, for a tag, `tag: v1`.
/// Case-insensitive; `needle` is lowercase.
pub fn ref_label_matches(label: &RefLabel, needle: &str) -> bool {
    let matches = |name: &str| name.to_lowercase().contains(needle);
    match label {
        RefLabel::Tag { name } => matches(&format!("tag: {name}")),
        RefLabel::Head => matches("HEAD"),
        RefLabel::Worktree(worktree) => matches(&worktree.name),
        RefLabel::Branch(branch) if !branch.local => matches(&branch.name),
        RefLabel::Branch(branch) => {
            matches(&branch.name)
                || branch
                    .remotes
                    .iter()
                    .any(|remote| matches(&format!("{remote}/{}", branch.name)))
                || (branch.current && matches("HEAD"))
                || branch
                    .worktree
                    .as_ref()
                    .is_some_and(|worktree| matches(&worktree.name))
        }
    }
}

/// The label in words, e.g. `main (checked out; local, origin, upstream)`, `v1 (tag)`,
/// `origin/feature (remote)`, `feature (local; checked out in gitto-feature)` or
/// `gitto-check (worktree, detached)`; for tooltips, where its icons aren't shown.
pub fn describe_ref_label(label: &RefLabel) -> String {
    let branch = match label {
        RefLabel::Tag { name } => return format!("{name} (tag)"),
        RefLabel::Head => return "HEAD (detached)".to_owned(),
        RefLabel::Worktree(worktree) => return format!("{} (worktree, detached)", worktree.name),
        RefLabel::Branch(branch) if !branch.local => return format!("{} (remote)", branch.name),
        RefLabel::Branch(branch) => branch,
    };
    let mut where_ = vec!["local"];
    where_.extend(branch.remotes.iter().map(String::as_str));
    let where_ = where_.join(", ");
    if let Some(worktree) = &branch.worktree {
        return format!(
            "{} ({where_}; checked out in {})",
            branch.name, worktree.name
        );
    }
    if branch.current {
        format!("{} (checked out; {where_})", branch.name)
    } else {
        format!("{} ({where_})", branch.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn worktree(name: &str, branch: Option<&str>, head: Option<&str>) -> Worktree {
        Worktree {
            path: format!("/w/{name}"),
            name: name.into(),
            head: head.map(str::to_owned),
            branch: branch.map(str::to_owned),
            main: false,
            bare: false,
            current: false,
            locked: None,
            prunable: None,
        }
    }

    /// The other worktrees: one on `feature`, one detached at `e5`, besides the current one on
    /// `main`.
    fn others() -> Checkouts {
        checkouts(&[
            Worktree {
                main: true,
                current: true,
                ..worktree("repo", Some("refs/heads/main"), Some("e5"))
            },
            worktree("repo-feature", Some("refs/heads/feature"), Some("e5")),
            worktree("repo-check", None, Some("e5")),
            worktree("repo-old", None, Some("a1")),
            Worktree {
                bare: true,
                ..worktree("bare.git", None, None)
            },
        ])
    }

    fn commit_ref(kind: CommitRefKind, name: &str, full_name: &str) -> CommitRef {
        CommitRef {
            kind,
            name: name.into(),
            full_name: full_name.into(),
            current: None,
        }
    }

    fn local(name: &str) -> CommitRef {
        commit_ref(CommitRefKind::Local, name, &format!("refs/heads/{name}"))
    }

    fn current(name: &str) -> CommitRef {
        CommitRef {
            current: Some(true),
            ..local(name)
        }
    }

    fn remote(name: &str) -> CommitRef {
        commit_ref(CommitRefKind::Remote, name, &format!("refs/remotes/{name}"))
    }

    fn tag(name: &str) -> CommitRef {
        commit_ref(CommitRefKind::Tag, name, &format!("refs/tags/{name}"))
    }

    fn head() -> CommitRef {
        commit_ref(CommitRefKind::Head, "HEAD", "HEAD")
    }

    fn branch(
        name: &str,
        full_name: &str,
        local: bool,
        remotes: &[&str],
        current: bool,
    ) -> RefLabel {
        RefLabel::Branch(BranchLabel {
            name: name.into(),
            full_name: full_name.into(),
            local,
            remotes: remotes.iter().map(|&remote| remote.to_owned()).collect(),
            current,
            worktree: None,
        })
    }

    fn labels(refs: &[CommitRef]) -> Vec<RefLabel> {
        to_ref_labels(refs, "", &Checkouts::default())
    }

    fn at(name: &str) -> WorktreeAt {
        WorktreeAt {
            name: name.into(),
            path: format!("/w/{name}"),
        }
    }

    mod to_ref_labels {
        use super::*;

        #[test]
        fn merges_a_local_branch_with_its_remote_copies_and_puts_the_checked_out_one_first() {
            assert_eq!(
                labels(&[
                    local("develop"),
                    current("main"),
                    remote("origin/main"),
                    remote("upstream/main"),
                    tag("v1"),
                ]),
                [
                    branch(
                        "main",
                        "refs/heads/main",
                        true,
                        &["origin", "upstream"],
                        true
                    ),
                    branch("develop", "refs/heads/develop", true, &[], false),
                    RefLabel::Tag { name: "v1".into() },
                ]
            );
        }

        #[test]
        fn keeps_a_remote_branch_without_a_local_copy_under_its_remotes_name() {
            assert_eq!(
                labels(&[remote("origin/feature/login")]),
                [branch(
                    "origin/feature/login",
                    "refs/remotes/origin/feature/login",
                    false,
                    &["origin"],
                    false
                )]
            );
        }

        #[test]
        fn matches_a_remote_branch_whose_name_has_slashes_to_its_local_branch() {
            assert_eq!(
                labels(&[remote("origin/feature/login"), local("feature/login")]),
                [branch(
                    "feature/login",
                    "refs/heads/feature/login",
                    true,
                    &["origin"],
                    false
                )]
            );
        }

        #[test]
        fn keeps_the_whole_name_of_a_remote_ref_without_a_remote_prefix() {
            assert_eq!(
                labels(&[remote("backup")]),
                [branch("backup", "refs/remotes/backup", false, &[], false)]
            );
        }

        #[test]
        fn shows_a_detached_head_on_its_own_even_where_a_branch_points_too() {
            assert_eq!(
                labels(&[head(), local("main")]),
                [
                    RefLabel::Head,
                    branch("main", "refs/heads/main", true, &[], false)
                ]
            );
        }

        #[test]
        fn keeps_a_local_branch_named_like_a_remote_one_apart_from_that_remote_branch() {
            let named_like_a_remote = CommitRef {
                current: Some(true),
                ..commit_ref(
                    CommitRefKind::Local,
                    "origin/main",
                    "refs/heads/origin/main",
                )
            };
            assert_eq!(
                labels(&[named_like_a_remote, remote("origin/main")]),
                [
                    branch("origin/main", "refs/heads/origin/main", true, &[], true),
                    branch(
                        "origin/main",
                        "refs/remotes/origin/main",
                        false,
                        &["origin"],
                        false
                    ),
                ]
            );
        }
    }

    mod to_ref_labels_with_other_worktrees {
        use super::*;

        #[test]
        fn says_which_worktree_a_branch_is_checked_out_in_and_names_the_ones_detached_at_the_commit()
         {
            let others = others();
            let labels = to_ref_labels(
                &[current("main"), local("feature"), remote("origin/feature")],
                "e5",
                &others,
            );
            let [main, feature, check] = &labels[..] else {
                panic!("three labels: {labels:?}")
            };
            assert!(
                matches!(main, RefLabel::Branch(b) if b.name == "main" && b.current),
                "{main:?}"
            );
            assert!(
                matches!(feature, RefLabel::Branch(b) if b.name == "feature" && b.worktree == Some(at("repo-feature"))),
                "{feature:?}"
            );
            assert_eq!(check, &RefLabel::Worktree(at("repo-check")));
            assert_eq!(
                to_ref_labels(&[], "a1", &others),
                [RefLabel::Worktree(at("repo-old"))]
            );
            assert_eq!(to_ref_labels(&[], "b2", &others), []);
        }

        #[test]
        fn leaves_out_the_worktree_on_show_and_a_bare_one() {
            let others = others();
            assert!(!others.branches.contains_key("refs/heads/main"));
            let detached: Vec<&str> = others
                .detached
                .values()
                .flatten()
                .map(|at| at.name.as_str())
                .collect();
            assert_eq!(detached, ["repo-check", "repo-old"]);
        }

        #[test]
        fn matches_and_describes_them() {
            let labels = to_ref_labels(&[current("main"), local("feature")], "e5", &others());
            let [_, feature, check] = &labels[..] else {
                panic!("three labels: {labels:?}")
            };
            assert!(ref_label_matches(check, "repo-ch"));
            assert!(ref_label_matches(feature, "repo-feat"));
            assert!(!ref_label_matches(feature, "head"));
            assert_eq!(describe_ref_label(check), "repo-check (worktree, detached)");
            assert_eq!(
                describe_ref_label(feature),
                "feature (local; checked out in repo-feature)"
            );
        }
    }

    mod ref_label_matches {
        use super::*;

        fn labels_() -> Vec<RefLabel> {
            labels(&[
                local("main"),
                remote("upstream/main"),
                remote("origin/feature"),
                tag("v1"),
            ])
        }

        #[test]
        fn matches_a_branch_by_its_name_or_a_remote_copys_full_name() {
            let labels = labels_();
            let [main, remote_only, _] = &labels[..] else {
                panic!("three labels: {labels:?}")
            };
            assert!(ref_label_matches(main, "main"));
            assert!(ref_label_matches(main, "upstream"));
            assert!(!ref_label_matches(main, "origin"));
            assert!(ref_label_matches(remote_only, "origin/feat"));
        }

        #[test]
        fn matches_a_tag_with_its_tag_prefix_case_insensitively() {
            let labels = labels_();
            assert!(ref_label_matches(&labels[2], "tag: v1"));
            assert!(ref_label_matches(
                &branch("Main", "refs/heads/Main", true, &[], false),
                "main"
            ));
        }
    }

    mod ref_label_matches_for_the_checked_out_branch {
        use super::*;

        #[test]
        fn matches_head_like_the_branchs_decoration_in_git() {
            let labels = labels(&[current("main")]);
            assert!(ref_label_matches(&labels[0], "head"));
        }
    }

    mod describe_ref_label {
        use super::*;

        #[test]
        fn spells_out_what_the_pills_icons_show() {
            let described: Vec<String> = labels(&[
                current("main"),
                remote("origin/main"),
                remote("origin/feature"),
                tag("v1"),
                head(),
            ])
            .iter()
            .map(describe_ref_label)
            .collect();
            assert_eq!(
                described,
                [
                    "HEAD (detached)",
                    "main (checked out; local, origin)",
                    "origin/feature (remote)",
                    "v1 (tag)",
                ]
            );
        }
    }
}
