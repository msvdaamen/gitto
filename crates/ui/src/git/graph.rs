//! Ported from `git/graph.ts`: the history's graph, laid out in lanes.

/// A line in one row of the graph, from lane `from` to lane `to` (the same lane for a straight one).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GraphEdge {
    pub from: usize,
    pub to: usize,
    /// Part of a dashed line from the uncommitted changes, or a stash, to the commit it's based on.
    pub dashed: bool,
}

/// One commit's row of the graph. Each row is drawn on its own: lines in the top half end at the
/// commit's node, lines in the bottom half start there, and lines passing by run top to bottom.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GraphRow {
    /// The lane the commit's node sits in; also its colour.
    pub column: usize,
    /// Lanes passing by the node, top to bottom (`from` and `to` are the same lane).
    pub through: Vec<GraphEdge>,
    /// Lines from the top of the row into the node: its own lane, and branches forked from it. Each
    /// takes the colour of the lane it comes from.
    pub top: Vec<GraphEdge>,
    /// Lines from the node to the bottom of the row: to its first parent, and to merged parents. Each
    /// takes the colour of the lane it goes to, so a merge is drawn in the merged branch's colour.
    pub bottom: Vec<GraphEdge>,
    /// How many lanes the row uses.
    pub width: usize,
}

/// A commit to lay out. `dashed` draws the line to its parent dashed, for uncommitted changes and
/// stashes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphCommit<'a> {
    pub sha: &'a str,
    pub parents: &'a [String],
    pub dashed: bool,
    /// Set aside from its parent's branch, like a stash: the parent stays in the lane of a branch
    /// that leads to it, if one does, rather than moving into this one's.
    pub aside: bool,
}

impl<'a> GraphCommit<'a> {
    /// A commit, neither dashed nor set aside.
    pub fn new(sha: &'a str, parents: &'a [String]) -> Self {
        Self {
            sha,
            parents,
            dashed: false,
            aside: false,
        }
    }
}

/// A lane: the commit it's heading towards (`None` for a free one), and whether it's drawn dashed,
/// from a `dashed` commit down to its parent, or comes from an `aside` one.
#[derive(Debug, Clone, Copy, Default)]
struct Lane<'a> {
    target: Option<&'a str>,
    dashed: bool,
    aside: bool,
}

/// Lays commits (newest first; children always before their parents) out in lanes, the way
/// GitKraken does: a branch keeps its lane from its tip down to where it forked off, merges curve
/// in from the merged branch's lane, and forks curve out of the commit they started at.
pub fn compute_graph(commits: &[GraphCommit<'_>]) -> Vec<GraphRow> {
    let mut lanes: Vec<Lane<'_>> = Vec::new();
    let mut incoming: Vec<usize> = Vec::new();

    commits
        .iter()
        .map(|commit| {
            let sha = commit.sha;
            incoming.clear();
            incoming.extend(
                lanes
                    .iter()
                    .enumerate()
                    .filter(|(_, lane)| lane.target == Some(sha))
                    .map(|(i, _)| i),
            );
            // A branch tip nothing leads to yet starts in the first free lane.
            let column = incoming
                .iter()
                .copied()
                .find(|&lane| !lanes[lane].aside)
                .or_else(|| incoming.first().copied())
                .unwrap_or_else(|| free_lane(&lanes, &[]));
            let through: Vec<GraphEdge> = lanes
                .iter()
                .enumerate()
                .filter(|(_, lane)| lane.target.is_some_and(|target| target != sha))
                .map(|(i, lane)| edge(i, i, lane.dashed))
                .collect();
            let top: Vec<GraphEdge> = incoming
                .iter()
                .map(|&lane| edge(lane, column, lanes[lane].dashed))
                .collect();

            // Branches forked from this commit end here. Their lanes free up only after this row,
            // so a merged parent below doesn't reuse a lane that is still drawn above the node.
            for &lane in &incoming {
                lanes[lane] = Lane::default();
            }
            let mut bottom = Vec::with_capacity(commit.parents.len());
            let mut parents = commit.parents.iter();
            if let Some(first) = parents.next() {
                set_lane(
                    &mut lanes,
                    column,
                    Lane {
                        target: Some(first.as_str()),
                        dashed: commit.dashed,
                        aside: commit.aside,
                    },
                );
                bottom.push(edge(column, column, commit.dashed));
            }
            for parent in parents {
                // Join a lane already heading to the parent, but not a dashed one from the
                // uncommitted changes or a stash: a merge drawn into it would look like part of
                // them.
                let joined = lanes
                    .iter()
                    .position(|lane| lane.target == Some(parent.as_str()) && !lane.dashed);
                let lane = joined.unwrap_or_else(|| {
                    let lane = free_lane(&lanes, &incoming);
                    set_lane(
                        &mut lanes,
                        lane,
                        Lane {
                            target: Some(parent.as_str()),
                            ..Lane::default()
                        },
                    );
                    lane
                });
                bottom.push(edge(column, lane, false));
            }
            while lanes.last().is_some_and(|lane| lane.target.is_none()) {
                lanes.pop();
            }

            let width = through
                .iter()
                .chain(&bottom)
                .map(|line| line.to)
                .chain(incoming.iter().copied())
                .fold(column, usize::max)
                + 1;
            GraphRow {
                column,
                through,
                top,
                bottom,
                width,
            }
        })
        .collect()
}

fn edge(from: usize, to: usize, dashed: bool) -> GraphEdge {
    GraphEdge { from, to, dashed }
}

/// Points lane `index` at `lane`, adding lanes up to it if there aren't as many.
fn set_lane<'a>(lanes: &mut Vec<Lane<'a>>, index: usize, lane: Lane<'a>) {
    if index >= lanes.len() {
        lanes.resize(index + 1, Lane::default());
    }
    lanes[index] = lane;
}

/// The first free lane, other than `reserved`; a new one past the others if none is.
fn free_lane(lanes: &[Lane<'_>], reserved: &[usize]) -> usize {
    lanes
        .iter()
        .enumerate()
        .position(|(i, lane)| lane.target.is_none() && !reserved.contains(&i))
        .unwrap_or(lanes.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A commit to lay out, as TS's tests write one: `("c", &["b"])`.
    struct Commit {
        sha: String,
        parents: Vec<String>,
        dashed: bool,
        aside: bool,
    }

    fn commit(sha: &str, parents: &[&str]) -> Commit {
        Commit {
            sha: sha.into(),
            parents: parents.iter().map(|&parent| parent.to_owned()).collect(),
            dashed: false,
            aside: false,
        }
    }

    fn dashed(sha: &str, parents: &[&str]) -> Commit {
        Commit {
            dashed: true,
            ..commit(sha, parents)
        }
    }

    fn graph(commits: &[Commit]) -> Vec<GraphRow> {
        let commits: Vec<GraphCommit<'_>> = commits
            .iter()
            .map(|commit| GraphCommit {
                sha: &commit.sha,
                parents: &commit.parents,
                dashed: commit.dashed,
                aside: commit.aside,
            })
            .collect();
        compute_graph(&commits)
    }

    fn line(from: usize, to: usize) -> GraphEdge {
        edge(from, to, false)
    }

    fn dashed_line(from: usize, to: usize) -> GraphEdge {
        edge(from, to, true)
    }

    mod compute_graph {
        use super::*;

        #[test]
        fn keeps_a_linear_history_in_a_single_lane() {
            let rows = graph(&[commit("c", &["b"]), commit("b", &["a"]), commit("a", &[])]);

            assert_eq!(
                rows,
                [
                    GraphRow {
                        column: 0,
                        through: vec![],
                        top: vec![],
                        bottom: vec![line(0, 0)],
                        width: 1
                    },
                    GraphRow {
                        column: 0,
                        through: vec![],
                        top: vec![line(0, 0)],
                        bottom: vec![line(0, 0)],
                        width: 1
                    },
                    GraphRow {
                        column: 0,
                        through: vec![],
                        top: vec![line(0, 0)],
                        bottom: vec![],
                        width: 1
                    },
                ]
            );
        }

        #[test]
        fn curves_a_merged_branch_in_from_its_own_lane_and_back_at_the_fork_point() {
            let rows = graph(&[
                commit("merge", &["main", "side"]),
                commit("main", &["base"]),
                commit("side", &["base"]),
                commit("base", &[]),
            ]);
            let [merge, main, side, base] = &rows[..] else {
                panic!("four rows")
            };

            // The merge opens a lane for the merged branch.
            assert_eq!(merge.bottom, [line(0, 0), line(0, 1)]);
            assert_eq!(main.column, 0);
            assert_eq!(main.through, [line(1, 1)]);
            assert_eq!(side.column, 1);
            assert_eq!(side.through, [line(0, 0)]);
            // Both lanes lead to the fork point, where the side branch's lane curves back in.
            assert_eq!(base.column, 0);
            assert_eq!(base.top, [line(0, 0), line(1, 0)]);
            assert_eq!(base.width, 2);
        }

        #[test]
        fn gives_a_branch_that_isnt_merged_its_own_lane_from_its_tip() {
            let rows = graph(&[
                commit("feature", &["base"]),
                commit("main", &["base"]),
                commit("base", &[]),
            ]);
            let [feature, main, base] = &rows[..] else {
                panic!("three rows")
            };

            assert_eq!(feature.column, 0);
            assert_eq!(feature.top, []);
            assert_eq!(main.column, 1);
            assert_eq!(main.top, []);
            assert_eq!(main.through, [line(0, 0)]);
            assert_eq!(base.top, [line(0, 0), line(1, 0)]);
        }

        #[test]
        fn joins_a_merged_parent_that_already_has_a_lane_instead_of_opening_another() {
            let rows = graph(&[
                commit("feature", &["side"]),
                commit("merge", &["main", "side"]),
                commit("side", &["main"]),
                commit("main", &[]),
            ]);
            let [feature, merge, ..] = &rows[..] else {
                panic!("four rows")
            };

            assert_eq!(feature.column, 0);
            assert_eq!(merge.column, 1);
            assert_eq!(merge.through, [line(0, 0)]);
            assert_eq!(merge.bottom, [line(1, 1), line(1, 0)]);
        }

        #[test]
        fn dashes_the_line_from_the_uncommitted_changes_all_the_way_down_to_their_commit() {
            let rows = graph(&[
                dashed("wip", &["head"]),
                commit("other", &["head"]),
                commit("head", &[]),
            ]);
            let [wip, other, head] = &rows[..] else {
                panic!("three rows")
            };

            assert_eq!(wip.bottom, [dashed_line(0, 0)]);
            // Commits on other branches can sort in between; the line passes them by still dashed.
            assert_eq!(other.through, [dashed_line(0, 0)]);
            assert_eq!(head.top, [dashed_line(0, 0), line(1, 0)]);
        }

        #[test]
        fn gives_a_merge_its_own_lane_instead_of_joining_the_dashed_one_to_the_same_commit() {
            let rows = graph(&[
                dashed("wip", &["head"]),
                commit("merge", &["base", "head"]),
                commit("base", &[]),
                commit("head", &[]),
            ]);
            let [_, merge, _, head] = &rows[..] else {
                panic!("four rows")
            };

            assert_eq!(merge.bottom, [line(1, 1), line(1, 2)]);
            assert_eq!(head.top, [dashed_line(0, 0), line(2, 0)]);
        }

        #[test]
        fn keeps_a_commit_in_its_branchs_lane_rather_than_a_stashs() {
            let rows = graph(&[
                commit("t", &["z"]),
                commit("c", &["b"]),
                commit("z", &[]),
                // Lane 0 is free again, so the stash takes it.
                Commit {
                    aside: true,
                    ..dashed("stash", &["b"])
                },
                commit("b", &[]),
            ]);
            let [_, _, _, stash, b] = &rows[..] else {
                panic!("five rows")
            };

            assert_eq!(stash.column, 0);
            // `b` stays in `c`'s lane; the stash's line curves over to it.
            assert_eq!(b.column, 1);
            assert_eq!(b.top, [dashed_line(0, 1), line(1, 1)]);
        }

        #[test]
        fn doesnt_reuse_a_lane_that_ends_at_a_node_for_that_nodes_merged_parent() {
            let rows = graph(&[
                commit("a", &["m"]),
                commit("b", &["m"]),
                commit("m", &["x", "y"]),
                commit("y", &["x"]),
                commit("x", &[]),
            ]);

            // Lane 1 ends at `m`, so its merged parent `y` goes to lane 2, not lane 1.
            assert!(rows[2].bottom.contains(&line(0, 2)));
        }
    }

    #[test]
    fn lays_out_a_long_history() {
        // 20,000 commits on 20 branches that fork off a trunk, then merge back in.
        let mut commits = Vec::new();
        for i in (0..20_000).rev() {
            let parents = if i == 0 {
                vec![]
            } else if i % 1000 == 999 {
                vec![format!("{}", i - 1), format!("{}", i - 500)]
            } else {
                vec![format!("{}", i - 1)]
            };
            commits.push(Commit {
                sha: i.to_string(),
                parents,
                dashed: false,
                aside: false,
            });
        }
        let rows = graph(&commits);
        assert_eq!(rows.len(), 20_000);
        assert!(rows.iter().all(|row| row.width <= 2));
    }
}
