//! Ported from `git/rows.ts`: the rows of the history table.

use gitto_git::features::history::schema::Commit;
use gitto_git::features::stash::schema::Stash;
use gitto_git::input::RepositoryId;
use std::collections::HashMap;

use super::graph::{GraphCommit, GraphRow, compute_graph};
use super::ref_labels::{Checkouts, RefLabel, to_ref_labels};
use super::short_sha;
use crate::format::{avatar_color, initials};

/// Which row of the history table, e.g. the one selected: the uncommitted changes', a stash's, or
/// a commit's, each by its full SHA. A stash's and a commit's are told apart by their kind, not
/// their SHA.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RowId {
    Wip,
    Stash(String),
    Commit(String),
}

impl RowId {
    /// The stash's or commit's SHA; `None` for the uncommitted changes.
    pub fn sha(&self) -> Option<&str> {
        match self {
            RowId::Wip => None,
            RowId::Stash(sha) | RowId::Commit(sha) => Some(sha),
        }
    }
}

/// The uncommitted changes' node in the history graph, which nodes are keyed by SHA in.
const WIP_NODE: &str = "wip";

/// What the uncommitted changes' row says, in place of a commit message.
pub const WIP_MESSAGE: &str = "Uncommitted changes";

/// A row in the history table: the uncommitted changes, a stash, or a commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryRow {
    Wip(WipRow),
    Stash(StashRow),
    Commit(CommitRow),
}

impl HistoryRow {
    /// The row's id.
    pub fn id(&self) -> RowId {
        match self {
            HistoryRow::Wip(_) => RowId::Wip,
            HistoryRow::Stash(row) => RowId::Stash(row.sha.clone()),
            HistoryRow::Commit(row) => RowId::Commit(row.id.clone()),
        }
    }

    /// Whether it's the row `id` is, without making its id.
    pub fn is(&self, id: &RowId) -> bool {
        match (self, id) {
            (HistoryRow::Wip(_), RowId::Wip) => true,
            (HistoryRow::Stash(row), RowId::Stash(sha)) => row.sha == *sha,
            (HistoryRow::Commit(row), RowId::Commit(sha)) => row.id == *sha,
            _ => false,
        }
    }

    /// Its row in the history graph, if it has one.
    pub fn graph(&self) -> Option<&GraphRow> {
        match self {
            HistoryRow::Wip(row) => row.graph.as_ref(),
            HistoryRow::Stash(row) => row.graph.as_ref(),
            HistoryRow::Commit(row) => row.graph.as_ref(),
        }
    }

    /// The repository it's in.
    pub fn repository_id(&self) -> &RepositoryId {
        match self {
            HistoryRow::Wip(row) => &row.repository_id,
            HistoryRow::Stash(row) => &row.repository_id,
            HistoryRow::Commit(row) => &row.repository_id,
        }
    }
}

/// The uncommitted changes in the working directory, above the commits. Its id is [`RowId::Wip`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WipRow {
    /// The repository the changes are in.
    pub repository_id: RepositoryId,
    /// Its row in the history graph.
    pub graph: Option<GraphRow>,
}

/// A stash, among the commits: above the one it was made on, with a dashed line to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StashRow {
    /// The stash commit's full SHA, its id's (see [`RowId::Stash`]).
    pub sha: String,
    /// The repository the stash is in.
    pub repository_id: RepositoryId,
    /// Its row in the history graph.
    pub graph: Option<GraphRow>,
    /// What git named it, e.g. `WIP on main: 1a2b3c4 Fix the header`.
    pub message: String,
    /// When it was made, in milliseconds since the epoch.
    pub created_at: i64,
}

/// A commit as the UI shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitRow {
    /// The full SHA.
    pub id: String,
    /// The repository the commit is in, so follow-up requests (like its files) go there.
    pub repository_id: RepositoryId,
    pub short_sha: String,
    /// Its row in the history graph; only rows in the history table have one.
    pub graph: Option<GraphRow>,
    pub message: String,
    pub description: Option<String>,
    pub author: String,
    pub initials: String,
    /// As `0xRRGGBB` (see [`avatar_color`]).
    pub avatar_color: u32,
    /// The commit date, which the history is sorted by, in milliseconds since the epoch.
    pub committed_at: i64,
    /// The branches and tags pointing at it (see [`to_ref_labels`]).
    pub labels: Vec<RefLabel>,
}

/// What the history lists below the uncommitted changes: the log's commits, and the stashes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HistoryEntry<'a> {
    Commit(&'a Commit),
    Stash(&'a Stash),
}

/// The log, newest first, with the stashes among its commits: each just above the commit it was made
/// on, where its line to it is one row long and so never pushes other branches aside, as a line
/// past their tips would. One made on a commit that isn't in the log (a deleted branch's, say) goes
/// by when it was made, without a line; one older than every commit in it is left out, as it belongs
/// further back than the log goes. Stashes above the same commit are newest first.
pub fn with_stashes<'a>(log: &'a [Commit], stashes: &'a [Stash]) -> Vec<HistoryEntry<'a>> {
    if stashes.is_empty() {
        return log.iter().map(HistoryEntry::Commit).collect();
    }
    let positions: HashMap<&str, usize> = log
        .iter()
        .enumerate()
        .map(|(index, commit)| (commit.sha.as_str(), index))
        .collect();
    // The stashes to list just above each commit.
    let mut above: HashMap<usize, Vec<&Stash>> = HashMap::new();
    // Newest first (as listed, unless the clock was turned back), so the first commit made no later
    // than each is found in one pass down the log: it's never above the one for a newer stash.
    let mut by_date: Vec<&Stash> = stashes.iter().collect();
    by_date.sort_by_key(|stash| std::cmp::Reverse(stash.created_at));
    let mut by_date_place = 0;
    for stash in by_date {
        while by_date_place < log.len() && log[by_date_place].committed_at > stash.created_at {
            by_date_place += 1;
        }
        let place = positions
            .get(stash.base.as_str())
            .copied()
            .unwrap_or(by_date_place);
        if place == log.len() {
            continue;
        }
        above.entry(place).or_default().push(stash);
    }
    let mut entries = Vec::with_capacity(log.len() + stashes.len());
    for (index, commit) in log.iter().enumerate() {
        if let Some(stashes) = above.remove(&index) {
            entries.extend(stashes.into_iter().map(HistoryEntry::Stash));
        }
        entries.push(HistoryEntry::Commit(commit));
    }
    entries
}

/// Lays out the history graph: the commits and stashes, below the uncommitted changes when there are
/// any. Those get a dashed line to the commit they're based on, `head`, and each stash one to the
/// commit it was made on, if it's in the history.
fn history_graph(
    entries: &[HistoryEntry<'_>],
    has_changes: bool,
    head: Option<&str>,
) -> Vec<GraphRow> {
    let shas: std::collections::HashSet<&str> = entries
        .iter()
        .filter_map(|entry| match entry {
            HistoryEntry::Commit(commit) => Some(commit.sha.as_str()),
            HistoryEntry::Stash(_) => None,
        })
        .collect();
    let head_parent: Vec<String> = head
        .filter(|head| shas.contains(head))
        .map(|head| vec![head.to_owned()])
        .unwrap_or_default();
    let mut commits: Vec<GraphCommit<'_>> =
        Vec::with_capacity(entries.len() + usize::from(has_changes));
    if has_changes {
        commits.push(wip_node(&head_parent));
    }
    commits.extend(entries.iter().map(|entry| match entry {
        HistoryEntry::Commit(commit) => GraphCommit::new(&commit.sha, &commit.parents),
        HistoryEntry::Stash(stash) => stash_node(stash, shas.contains(stash.base.as_str())),
    }));
    compute_graph(&commits)
}

/// The uncommitted changes' node in the history graph: a dashed line to `head_parent`, the commit
/// they're based on if it's in the history (else none).
pub fn wip_node(head_parent: &[String]) -> GraphCommit<'_> {
    GraphCommit {
        dashed: true,
        ..GraphCommit::new(WIP_NODE, head_parent)
    }
}

/// A stash's node in the history graph: set aside, with a dashed line to the commit it was made
/// on if that's in the history (`based`).
pub fn stash_node(stash: &Stash, based: bool) -> GraphCommit<'_> {
    GraphCommit {
        sha: &stash.sha,
        parents: if based {
            std::slice::from_ref(&stash.base)
        } else {
            &[]
        },
        dashed: true,
        aside: true,
    }
}

/// Turns a repository's commits and stashes, plus the uncommitted changes if there are any, into
/// history table rows, each with its row of the history graph, which is laid out for them: the
/// uncommitted changes are based on `head`. The commits' labels say where the other worktrees are
/// checked out (`others`), and which of the `remotes` their remote branches are on.
pub fn to_history_rows<S: AsRef<str>>(
    repository_id: &RepositoryId,
    entries: &[HistoryEntry<'_>],
    has_changes: bool,
    head: Option<&str>,
    others: &Checkouts,
    remotes: &[S],
) -> Vec<HistoryRow> {
    let mut graph = history_graph(entries, has_changes, head).into_iter();
    let mut rows = Vec::with_capacity(entries.len() + usize::from(has_changes));
    if has_changes {
        rows.push(HistoryRow::Wip(WipRow {
            repository_id: repository_id.clone(),
            graph: graph.next(),
        }));
    }
    rows.extend(entries.iter().map(|entry| match entry {
        HistoryEntry::Commit(commit) => HistoryRow::Commit(to_commit_row(
            repository_id,
            commit,
            graph.next(),
            others,
            remotes,
        )),
        HistoryEntry::Stash(stash) => {
            HistoryRow::Stash(to_stash_row(repository_id, stash, graph.next()))
        }
    }));
    rows
}

/// A stash's row.
pub fn to_stash_row(
    repository_id: &RepositoryId,
    stash: &Stash,
    graph: Option<GraphRow>,
) -> StashRow {
    StashRow {
        sha: stash.sha.clone(),
        repository_id: repository_id.clone(),
        graph,
        message: stash.message.clone(),
        created_at: stash.created_at,
    }
}

/// A commit's row; its labels say where the other worktrees are checked out (`others`), and which
/// of the `remotes` its remote branches are on.
pub fn to_commit_row<S: AsRef<str>>(
    repository_id: &RepositoryId,
    commit: &Commit,
    graph: Option<GraphRow>,
    others: &Checkouts,
    remotes: &[S],
) -> CommitRow {
    CommitRow {
        id: commit.sha.clone(),
        repository_id: repository_id.clone(),
        short_sha: short_sha(&commit.sha).to_owned(),
        graph,
        message: commit.subject.clone(),
        description: (!commit.body.is_empty()).then(|| commit.body.clone()),
        author: commit.author_name.clone(),
        initials: initials(&commit.author_name),
        avatar_color: avatar_color(&commit.author_email),
        committed_at: commit.committed_at,
        labels: to_ref_labels(&commit.refs, &commit.sha, others, remotes),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::graph::GraphEdge;

    fn commit(sha: &str, parents: &[&str], committed_at: i64) -> Commit {
        Commit {
            sha: sha.into(),
            parents: parents.iter().map(|&parent| parent.to_owned()).collect(),
            author_name: "Ada Lovelace".into(),
            author_email: "ada@example.com".into(),
            authored_at: committed_at,
            committed_at,
            refs: vec![],
            subject: sha.into(),
            body: String::new(),
        }
    }

    fn stash(sha: &str, base: &str, created_at: i64) -> Stash {
        Stash {
            sha: sha.into(),
            base: base.into(),
            message: format!("WIP {sha}"),
            created_at,
        }
    }

    /// The SHAs of `with_stashes`'s entries, in order.
    fn order(log: &[Commit], stashes: &[Stash]) -> Vec<String> {
        with_stashes(log, stashes)
            .iter()
            .map(|entry| match entry {
                HistoryEntry::Commit(commit) => commit.sha.clone(),
                HistoryEntry::Stash(stash) => stash.sha.clone(),
            })
            .collect()
    }

    fn log() -> Vec<Commit> {
        vec![
            commit("c", &["b"], 3000),
            commit("b", &["a"], 2000),
            commit("a", &[], 1000),
        ]
    }

    fn line(from: usize, to: usize, dashed: bool) -> GraphEdge {
        GraphEdge { from, to, dashed }
    }

    mod with_stashes {
        use super::*;

        #[test]
        fn lists_each_stash_just_above_the_commit_it_was_made_on_whenever_it_was_made() {
            // Made after `c` was committed, but on `b`: no line runs past `c`'s tip.
            assert_eq!(
                order(&log(), &[stash("s2", "b", 9000), stash("s1", "a", 1500)]),
                ["c", "s2", "b", "s1", "a"]
            );
            // A clock that was behind, or a commit amended to a later date.
            assert_eq!(order(&log(), &[stash("s", "c", 500)]), ["s", "c", "b", "a"]);
        }

        #[test]
        fn lists_stashes_on_the_same_commit_newest_first() {
            // Listed out of date order, as when the clock was turned back between them.
            assert_eq!(
                order(
                    &log(),
                    &[stash("older", "b", 2100), stash("newer", "b", 2500)]
                ),
                ["c", "newer", "older", "b", "a"]
            );
        }

        #[test]
        fn goes_by_the_date_for_a_stash_made_on_a_commit_that_isnt_in_the_log() {
            assert_eq!(
                order(&log(), &[stash("s", "gone", 2500)]),
                ["c", "s", "b", "a"]
            );
            // Made in the same second as a commit: above it.
            assert_eq!(
                order(&log(), &[stash("s", "gone", 2000)]),
                ["c", "s", "b", "a"]
            );
            // Older than every commit in the log: further back than it goes.
            assert_eq!(order(&log(), &[stash("s", "gone", 10)]), ["c", "b", "a"]);
        }
    }

    mod history_graph {
        use super::*;

        #[test]
        fn draws_a_dashed_line_from_each_stash_to_the_commit_it_was_made_on_if_its_listed() {
            let log = log();
            let stashes = [stash("s", "b", 2500), stash("t", "gone", 2600)];
            let entries = with_stashes(&log, &stashes);
            let graph = history_graph(&entries, false, Some("c"));
            let [_, t, s, b, ..] = &graph[..] else {
                panic!("five rows: {graph:?}")
            };
            // In a lane of its own, next to the one from `c`.
            assert_eq!(s.column, 1);
            assert_eq!(s.bottom, [line(1, 1, true)]);
            assert_eq!(t.bottom, []);
            assert_eq!(b.top, [line(0, 0, false), line(1, 0, true)]);
        }

        #[test]
        fn keeps_the_branches_in_their_lanes_when_a_stash_was_made_on_an_older_commit() {
            // Stashed on `b`, then `c` was pulled: `c`'s tip still starts the first lane.
            let log = log();
            let stashes = [stash("s", "b", 9000)];
            let graph = history_graph(&with_stashes(&log, &stashes), false, Some("c"));
            let columns: Vec<usize> = graph.iter().map(|row| row.column).collect();
            assert_eq!(columns, [0, 1, 0, 0]);
            assert_eq!(graph.iter().map(|row| row.width).max(), Some(2));
        }

        #[test]
        fn gives_the_stashes_rows_of_their_own_below_the_uncommitted_changes() {
            let log = log();
            let stashes = [stash("s", "b", 2500)];
            let entries = with_stashes(&log, &stashes);
            let repository = RepositoryId::generate();
            let rows = to_history_rows(
                &repository,
                &entries,
                true,
                Some("c"),
                &Checkouts::default(),
                &["origin"],
            );
            let ids: Vec<RowId> = rows.iter().map(HistoryRow::id).collect();
            assert_eq!(
                ids,
                [
                    RowId::Wip,
                    RowId::Commit("c".into()),
                    RowId::Stash("s".into()),
                    RowId::Commit("b".into()),
                    RowId::Commit("a".into()),
                ]
            );
            assert!(rows[2].is(&RowId::Stash("s".into())));
            assert!(!rows[2].is(&RowId::Commit("s".into())));
            assert_eq!(ids[2].sha(), Some("s"));
            assert_eq!(ids[0].sha(), None);
            let HistoryRow::Stash(row) = &rows[2] else {
                panic!("a stash: {:?}", rows[2])
            };
            assert_eq!((row.sha.as_str(), row.message.as_str()), ("s", "WIP s"));
            assert_eq!(row.graph.as_ref().map(|graph| graph.column), Some(1));
        }
    }

    #[test]
    fn shows_a_commit_with_its_author_and_labels() {
        let repository = RepositoryId::generate();
        let commit = Commit {
            body: "Details".into(),
            ..commit("1a2b3c4d5e", &[], 1000)
        };
        let row = to_commit_row(
            &repository,
            &commit,
            None,
            &Checkouts::default(),
            &["origin"],
        );
        assert_eq!(row.short_sha, "1a2b3c4");
        assert_eq!(row.initials, "AL");
        assert_eq!(row.description.as_deref(), Some("Details"));
    }
}
