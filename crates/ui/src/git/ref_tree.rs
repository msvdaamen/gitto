//! Ported from `git/ref-tree.ts`: the sidebar's branches, remote branches and tags, in folders.

use std::collections::HashMap;

use gitto_git::features::refs::schema::Ref;

/// A folder of refs, or a ref.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefTreeNode {
    Folder(RefFolder),
    Ref(RefLeaf),
}

impl RefTreeNode {
    /// The folder's name, or the ref's last path segment.
    pub fn name(&self) -> &str {
        match self {
            RefTreeNode::Folder(folder) => &folder.name,
            RefTreeNode::Ref(leaf) => &leaf.name,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefFolder {
    pub name: String,
    /// Full ref prefix of the folder, e.g. `refs/remotes/origin/feature`. Unique across kinds.
    pub path: String,
    /// Number of refs anywhere below this folder.
    pub count: usize,
    pub children: Vec<RefTreeNode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefLeaf {
    /// Last path segment of the ref's short name, e.g. `login` for `feature/login`.
    pub name: String,
    pub r#ref: Ref,
}

/// A folder as the tree is built: its children, by where they are.
struct Building {
    name: String,
    path: String,
    count: usize,
    children: Vec<Child>,
}

#[derive(Clone, Copy)]
enum Child {
    /// A folder, by its index among the folders.
    Folder(usize),
    /// A ref, by its index among the refs.
    Ref(usize),
}

/// Groups refs into folders on `/`, e.g. `feature/login` → folder `feature` with ref `login`, and
/// `origin/feature/login` → `origin` › `feature` › `login`. Folders come before refs at each level;
/// otherwise the input order is kept.
pub fn build_ref_tree(refs: &[Ref]) -> Vec<RefTreeNode> {
    // The root first; the others by path, so a ref's folders are looked up rather than searched for
    // among their siblings, of which a remote can have thousands.
    let mut folders = vec![Building {
        name: String::new(),
        path: String::new(),
        count: 0,
        children: Vec::new(),
    }];
    let mut by_path: HashMap<String, usize> = HashMap::new();

    for (index, r) in refs.iter().enumerate() {
        // The full name up to the short one, e.g. `refs/remotes` for `origin/main`.
        let prefix = r
            .full_name
            .get(..r.full_name.len().saturating_sub(r.name.len()))
            .unwrap_or("");
        let mut path = prefix.strip_suffix('/').unwrap_or(prefix).to_owned();
        let mut folder = 0;
        folders[folder].count += 1;

        if let Some((segments, _)) = r.name.rsplit_once('/') {
            for segment in segments.split('/') {
                path.push('/');
                path.push_str(segment);
                let child = match by_path.get(&path) {
                    Some(&child) => child,
                    None => {
                        let child = folders.len();
                        folders.push(Building {
                            name: segment.to_owned(),
                            path: path.clone(),
                            count: 0,
                            children: Vec::new(),
                        });
                        by_path.insert(path.clone(), child);
                        folders[folder].children.push(Child::Folder(child));
                        child
                    }
                };
                folders[child].count += 1;
                folder = child;
            }
        }

        folders[folder].children.push(Child::Ref(index));
    }

    let mut folders: Vec<Option<Building>> = folders.into_iter().map(Some).collect();
    let root = folders[0].take().expect("the root");
    sort_folders_first(root.children, &mut folders, refs)
}

/// The nodes of `children`, folders before refs, each folder's own sorted the same way.
fn sort_folders_first(
    children: Vec<Child>,
    folders: &mut [Option<Building>],
    refs: &[Ref],
) -> Vec<RefTreeNode> {
    let mut nodes = Vec::with_capacity(children.len());
    for child in &children {
        if let Child::Folder(index) = *child {
            let folder = folders[index].take().expect("each folder is in one place");
            nodes.push(RefTreeNode::Folder(RefFolder {
                children: sort_folders_first(folder.children, folders, refs),
                name: folder.name,
                path: folder.path,
                count: folder.count,
            }));
        }
    }
    for child in children {
        if let Child::Ref(index) = child {
            let r = &refs[index];
            let name = r.name.rsplit('/').next().unwrap_or(&r.name);
            nodes.push(RefTreeNode::Ref(RefLeaf {
                name: name.to_owned(),
                r#ref: r.clone(),
            }));
        }
    }
    nodes
}

/// A line of the tree as the sidebar shows it: a folder or ref, indented `depth` levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefTreeRow<'a> {
    pub node: &'a RefTreeNode,
    pub depth: usize,
}

/// The tree as the flat list of lines it shows, top to bottom; the children of collapsed folders
/// (`is_collapsed` takes a folder's `path`) are left out. Flat so the sidebar can render only the
/// lines in view.
pub fn flatten_ref_tree<'a>(
    nodes: &'a [RefTreeNode],
    is_collapsed: impl Fn(&str) -> bool,
) -> Vec<RefTreeRow<'a>> {
    fn visit<'a>(
        level: &'a [RefTreeNode],
        depth: usize,
        is_collapsed: &dyn Fn(&str) -> bool,
        rows: &mut Vec<RefTreeRow<'a>>,
    ) {
        for node in level {
            rows.push(RefTreeRow { node, depth });
            if let RefTreeNode::Folder(folder) = node
                && !is_collapsed(&folder.path)
            {
                visit(&folder.children, depth + 1, is_collapsed, rows);
            }
        }
    }
    let mut rows = Vec::new();
    visit(nodes, 0, &is_collapsed, &mut rows);
    rows
}

#[cfg(test)]
mod tests {
    use gitto_git::features::refs::schema::RefKind;

    use super::*;

    fn r(full_name: &str, name: &str, kind: RefKind) -> Ref {
        Ref {
            name: name.into(),
            full_name: full_name.into(),
            kind,
            sha: String::new(),
            current: false,
            upstream: None,
            ahead: 0,
            behind: 0,
        }
    }

    fn local(name: &str) -> Ref {
        r(&format!("refs/heads/{name}"), name, RefKind::Local)
    }

    fn remote(name: &str) -> Ref {
        r(&format!("refs/remotes/{name}"), name, RefKind::Remote)
    }

    /// The tree as nested names, with a trailing `/` on folders: `feature/[ui/[button] login]`.
    fn shape(nodes: &[RefTreeNode]) -> String {
        nodes
            .iter()
            .map(|node| match node {
                RefTreeNode::Folder(folder) => {
                    format!("{}/[{}]", folder.name, shape(&folder.children))
                }
                RefTreeNode::Ref(leaf) => leaf.name.clone(),
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    mod build_ref_tree {
        use super::*;

        #[test]
        fn keeps_branches_without_a_slash_at_the_top_level() {
            assert_eq!(
                shape(&build_ref_tree(&[local("main"), local("dev")])),
                "main dev"
            );
        }

        #[test]
        fn groups_branches_on_the_part_before_each_slash_folders_first() {
            let tree = build_ref_tree(&[
                local("main"),
                local("feature/login"),
                local("fix/crash"),
                local("feature/ui/button"),
            ]);

            assert_eq!(shape(&tree), "feature/[ui/[button] login] fix/[crash] main");
        }

        #[test]
        fn nests_remote_branches_under_their_remote() {
            let tree = build_ref_tree(&[remote("origin/main"), remote("origin/feature/login")]);

            assert_eq!(shape(&tree), "origin/[feature/[login] main]");
        }

        #[test]
        fn gives_folders_their_full_ref_path_and_a_count_of_the_refs_below_them() {
            let tree = build_ref_tree(&[remote("origin/main"), remote("origin/feature/login")]);
            let [RefTreeNode::Folder(origin)] = &tree[..] else {
                panic!("one folder: {tree:?}")
            };

            assert_eq!(
                (origin.path.as_str(), origin.count),
                ("refs/remotes/origin", 2)
            );
            let RefTreeNode::Folder(feature) = &origin.children[0] else {
                panic!("a folder first: {origin:?}")
            };
            assert_eq!(
                (feature.path.as_str(), feature.count),
                ("refs/remotes/origin/feature", 1)
            );
        }

        #[test]
        fn keeps_a_branch_and_a_folder_with_the_same_name_apart() {
            assert_eq!(
                shape(&build_ref_tree(&[local("feature"), local("feature/login")])),
                "feature/[login] feature"
            );
        }
    }

    mod flatten_ref_tree {
        use super::*;

        fn tree() -> Vec<RefTreeNode> {
            build_ref_tree(&[
                local("main"),
                local("feature/login"),
                local("feature/ui/button"),
                local("fix/crash"),
            ])
        }

        fn lines(collapsed: &[&str]) -> Vec<String> {
            let tree = tree();
            flatten_ref_tree(&tree, |path| collapsed.contains(&path))
                .iter()
                .map(|row| {
                    let slash = if matches!(row.node, RefTreeNode::Folder(_)) {
                        "/"
                    } else {
                        ""
                    };
                    format!("{}{}{slash}", "  ".repeat(row.depth), row.node.name())
                })
                .collect()
        }

        #[test]
        fn lists_folders_before_their_children_indented_by_depth() {
            assert_eq!(
                lines(&[]),
                [
                    "feature/",
                    "  ui/",
                    "    button",
                    "  login",
                    "fix/",
                    "  crash",
                    "main"
                ]
            );
        }

        #[test]
        fn leaves_out_whats_inside_collapsed_folders() {
            assert_eq!(
                lines(&["refs/heads/feature"]),
                ["feature/", "fix/", "  crash", "main"]
            );
            assert_eq!(
                lines(&["refs/heads/feature/ui"]),
                ["feature/", "  ui/", "  login", "fix/", "  crash", "main"]
            );
        }
    }
}
