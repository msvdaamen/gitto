//! The history's rows, laid out from what they're made of as `git/rows.ts` lays them out
//! (`with_stashes` and `to_history_rows`), but a part at a time, so a change lays out again only
//! the parts it changed: a page loaded is laid out on its own, from where the graph's lanes were
//! left by the rows before it, and a part whose commits, stashes and lanes are as they were is
//! kept. The uncommitted changes' row comes first, on its own; the lanes it leaves change the
//! rows below it only down to where the graph's back as it was without it, often the first
//! commit.
//!
//! Laying out is pure, and its result shared, so the table does it off the main thread (see
//! `HistoryTable::lay_out`).

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use gitto_app_data::data::history::LogCommits;
use gitto_git::features::history::schema::Commit;
use gitto_git::features::stash::schema::Stash;
use gitto_git::features::worktree::schema::Worktree;
use gitto_git::input::RepositoryId;
use gitto_ui_logic::git::graph::{GraphCommit, GraphLanes};
use gitto_ui_logic::git::ref_labels::checkouts;
use gitto_ui_logic::git::rows::{
    HistoryRow, RowId, WipRow, stash_node, to_commit_row, to_stash_row, wip_node,
};

/// The most commits in a part: what a has-changes flip lays out again, at the least.
const PART: usize = 1000;

/// What the rows are made of: the log's commits (`None` before it's loaded), the stashes among
/// them, a row of the uncommitted changes if there are any, based on `head`, and on their labels
/// the other worktrees' checkouts and the remotes their remote branches are on.
#[derive(Clone)]
pub struct LayoutInputs {
    pub repository_id: RepositoryId,
    pub commits: Option<LogCommits>,
    pub stashes: Arc<[Stash]>,
    pub worktrees: Arc<[Worktree]>,
    pub has_changes: bool,
    pub head: Option<String>,
    pub remotes: Vec<String>,
}

impl LayoutInputs {
    /// Inputs of nothing: before the history's loaded.
    pub fn none(repository_id: RepositoryId) -> Self {
        LayoutInputs {
            repository_id,
            commits: None,
            stashes: Arc::from(Vec::new()),
            worktrees: Arc::from(Vec::new()),
            has_changes: false,
            head: None,
            remotes: Vec::new(),
        }
    }

    /// Whether the rows laid out from these are those laid out from `other`, as far as can be
    /// told without comparing commits: the same pages of the log, the same lists.
    pub fn is(&self, other: &LayoutInputs) -> bool {
        self.repository_id == other.repository_id
            && match (&self.commits, &other.commits) {
                (Some(a), Some(b)) => a.is(b),
                (None, None) => true,
                _ => false,
            }
            && same(&self.stashes, &other.stashes)
            && same(&self.worktrees, &other.worktrees)
            && self.has_changes == other.has_changes
            && self.head == other.head
            && self.remotes == other.remotes
    }

    /// Whether the commits' rows say the same of their labels, from these and `other`.
    fn labels_as(&self, other: &LayoutInputs) -> bool {
        self.repository_id == other.repository_id
            && same(&self.worktrees, &other.worktrees)
            && self.remotes == other.remotes
    }
}

/// Whether two shared lists are the same: the same list, or both empty (what's shown of one
/// that hasn't loaded).
fn same<T>(a: &Arc<[T]>, b: &Arc<[T]>) -> bool {
    Arc::ptr_eq(a, b) || (a.is_empty() && b.is_empty())
}

/// Up to `PART` commits of a page of the log, and where each is among them, by its SHA.
struct PartCommits {
    page: Arc<[Commit]>,
    range: Range<usize>,
    at: HashMap<String, usize>,
}

impl PartCommits {
    fn new(page: &Arc<[Commit]>, range: Range<usize>) -> Self {
        let mut at = HashMap::with_capacity(range.len());
        for (ix, commit) in page[range.clone()].iter().enumerate() {
            at.entry(commit.sha.clone()).or_insert(ix);
        }
        PartCommits {
            page: Arc::clone(page),
            range,
            at,
        }
    }

    fn is(&self, page: &Arc<[Commit]>, range: &Range<usize>) -> bool {
        Arc::ptr_eq(&self.page, page) && self.range == *range
    }

    fn commits(&self) -> &[Commit] {
        &self.page[self.range.clone()]
    }
}

/// A stash among a part's commits: above which of them, and whether it has a line to the one it
/// was made on, which it does if that's in the history.
#[derive(Clone, PartialEq)]
struct Placed {
    above: usize,
    stash: Stash,
    based: bool,
}

/// A part's rows: its commits', with the stashes among them, and the graph's lanes before them
/// and after.
struct Part {
    commits: Arc<PartCommits>,
    /// By the commit each is above, in order.
    stashes: Vec<Placed>,
    before: GraphLanes,
    after: GraphLanes,
    rows: Vec<HistoryRow>,
    /// The most lanes a row uses.
    lanes: usize,
}

impl Part {
    /// The place among the part's rows of the row `id`, if it's here.
    fn place(&self, id: &RowId) -> Option<usize> {
        match id {
            RowId::Wip => None,
            RowId::Commit(sha) => {
                let ix = *self.commits.at.get(sha)?;
                Some(ix + self.stashes.partition_point(|placed| placed.above <= ix))
            }
            RowId::Stash(sha) => self
                .stashes
                .iter()
                .position(|placed| placed.stash.sha == *sha)
                .map(|nth| self.stashes[nth].above + nth),
        }
    }
}

/// The history's rows, as laid out from [`LayoutInputs`].
pub struct HistoryLayout {
    inputs: LayoutInputs,
    wip: Option<HistoryRow>,
    parts: Vec<Arc<Part>>,
    /// Where each part's rows start among all the rows.
    starts: Vec<usize>,
    len: usize,
    lanes: usize,
}

impl HistoryLayout {
    /// No rows, before any are laid out.
    pub fn empty(repository_id: RepositoryId) -> Self {
        HistoryLayout {
            inputs: LayoutInputs::none(repository_id),
            wip: None,
            parts: Vec::new(),
            starts: Vec::new(),
            len: 0,
            lanes: 1,
        }
    }

    /// What the rows were laid out from.
    pub fn inputs(&self) -> &LayoutInputs {
        &self.inputs
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The most lanes a row's graph uses, which the graph's column is sized to.
    pub fn lanes(&self) -> usize {
        self.lanes
    }

    pub fn get(&self, ix: usize) -> Option<&HistoryRow> {
        if let Some(wip) = &self.wip
            && ix == 0
        {
            return Some(wip);
        }
        let part = self
            .starts
            .partition_point(|start| *start <= ix)
            .checked_sub(1)?;
        self.parts[part].rows.get(ix - self.starts[part])
    }

    pub fn iter(&self) -> impl Iterator<Item = &HistoryRow> {
        self.wip
            .iter()
            .chain(self.parts.iter().flat_map(|part| part.rows.iter()))
    }

    /// The place among the rows of the row `id`, if it's there.
    pub fn position(&self, id: &RowId) -> Option<usize> {
        if *id == RowId::Wip {
            return self.wip.is_some().then_some(0);
        }
        self.parts
            .iter()
            .zip(&self.starts)
            .find_map(|(part, start)| Some(start + part.place(id)?))
    }

    /// The rows laid out from `inputs`, keeping those of `previous`'s parts that come out the
    /// same.
    pub fn lay_out(previous: &HistoryLayout, inputs: LayoutInputs) -> HistoryLayout {
        let Some(log) = inputs.commits.clone() else {
            return HistoryLayout {
                inputs,
                ..HistoryLayout::empty(previous.inputs.repository_id.clone())
            };
        };
        let labels_kept = inputs.labels_as(&previous.inputs);
        let commits = parts_of(&log, previous);
        let total: usize = commits.iter().map(|part| part.range.len()).sum();
        let first_commits: Vec<usize> = commits
            .iter()
            .scan(0, |start, part| {
                let first = *start;
                *start += part.range.len();
                Some(first)
            })
            .collect();
        // Where the commit `sha` is: its part, and its place in it.
        let find = |sha: &str| {
            commits
                .iter()
                .enumerate()
                .find_map(|(part, commits)| Some((part, *commits.at.get(sha)?)))
        };

        let mut lanes = GraphLanes::default();
        let wip = inputs.has_changes.then(|| {
            let head_parent: Vec<String> = inputs
                .head
                .iter()
                .filter(|head| find(head).is_some())
                .cloned()
                .collect();
            HistoryRow::Wip(WipRow {
                repository_id: inputs.repository_id.clone(),
                graph: Some(lanes.lay_out(&wip_node(&head_parent))),
            })
        });

        // The stashes among the commits, as `with_stashes` lists them: newest first, each just
        // above the commit it was made on, or the first made no later than it if that's not in
        // the log, or left out if it's older than every commit in it.
        let mut placed: Vec<Vec<Placed>> = vec![Vec::new(); commits.len()];
        let mut by_date: Vec<&Stash> = inputs.stashes.iter().collect();
        by_date.sort_by_key(|stash| std::cmp::Reverse(stash.created_at));
        let mut dates = log.iter().map(|commit| commit.committed_at).peekable();
        let mut by_date_place = 0;
        for stash in by_date {
            while dates.next_if(|date| *date > stash.created_at).is_some() {
                by_date_place += 1;
            }
            let base = find(&stash.base);
            let place = match base {
                Some((part, ix)) => first_commits[part] + ix,
                None => by_date_place,
            };
            if place == total {
                continue;
            }
            let part = first_commits.partition_point(|first| *first <= place) - 1;
            placed[part].push(Placed {
                above: place - first_commits[part],
                stash: stash.clone(),
                based: base.is_some(),
            });
        }

        let others = checkouts(&inputs.worktrees);
        let mut parts = Vec::with_capacity(commits.len());
        for (ix, (commits, mut stashes)) in commits.into_iter().zip(placed).enumerate() {
            stashes.sort_by_key(|placed| placed.above);
            let kept = previous.parts.get(ix).filter(|part| {
                labels_kept
                    && Arc::ptr_eq(&part.commits, &commits)
                    && part.stashes == stashes
                    && part.before == lanes
            });
            let part = match kept {
                Some(part) => Arc::clone(part),
                None => {
                    let before = lanes.clone();
                    let mut rows = Vec::with_capacity(commits.range.len() + stashes.len());
                    let mut above = stashes.iter().peekable();
                    for (ix, commit) in commits.commits().iter().enumerate() {
                        while let Some(placed) = above.next_if(|placed| placed.above == ix) {
                            let graph = lanes.lay_out(&stash_node(&placed.stash, placed.based));
                            rows.push(HistoryRow::Stash(to_stash_row(
                                &inputs.repository_id,
                                &placed.stash,
                                Some(graph),
                            )));
                        }
                        let graph = lanes.lay_out(&GraphCommit::new(&commit.sha, &commit.parents));
                        rows.push(HistoryRow::Commit(to_commit_row(
                            &inputs.repository_id,
                            commit,
                            Some(graph),
                            &others,
                            &inputs.remotes,
                        )));
                    }
                    let widest = rows
                        .iter()
                        .map(|row| row.graph().map_or(1, |graph| graph.width))
                        .max()
                        .unwrap_or(1);
                    Arc::new(Part {
                        commits,
                        stashes,
                        before,
                        after: lanes.clone(),
                        rows,
                        lanes: widest,
                    })
                }
            };
            lanes = part.after.clone();
            parts.push(part);
        }

        let mut len = usize::from(wip.is_some());
        let starts = parts
            .iter()
            .map(|part| {
                let start = len;
                len += part.rows.len();
                start
            })
            .collect();
        let lanes = wip
            .iter()
            .filter_map(HistoryRow::graph)
            .map(|graph| graph.width)
            .chain(parts.iter().map(|part| part.lanes))
            .max()
            .unwrap_or(1)
            .max(1);
        HistoryLayout {
            inputs,
            wip,
            parts,
            starts,
            len,
            lanes,
        }
    }
}

/// The log's commits in parts: a page's, up to `PART` at a time, kept from `previous` where it
/// has the same.
fn parts_of(log: &LogCommits, previous: &HistoryLayout) -> Vec<Arc<PartCommits>> {
    let mut parts = Vec::new();
    for page in log.pages() {
        for start in (0..page.len()).step_by(PART) {
            let range = start..(start + PART).min(page.len());
            let ix = parts.len();
            let part = match previous.parts.get(ix) {
                Some(part) if part.commits.is(page, &range) => Arc::clone(&part.commits),
                _ => Arc::new(PartCommits::new(page, range)),
            };
            parts.push(part);
        }
    }
    parts
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use gitto_git::features::history::schema::{CommitRef, CommitRefKind};
    use gitto_ui_logic::git::rows::{to_history_rows, with_stashes};

    use super::*;

    fn repository_id() -> RepositoryId {
        RepositoryId::parse("01890a5d-ac96-774b-bcce-b302099a8057").unwrap()
    }

    fn sha(ix: usize) -> String {
        format!("{ix:040x}")
    }

    /// `count` commits, newest first, a second apart: a trunk with a branch forking off and
    /// merging back every 50 commits, and a tag every 7.
    fn history(count: usize) -> Vec<Commit> {
        (0..count)
            .map(|ix| {
                let parent = |at: usize| (at < count).then(|| sha(at));
                let parents = if ix % 50 == 0 {
                    [parent(ix + 1), parent(ix + 20)]
                        .into_iter()
                        .flatten()
                        .collect()
                } else {
                    parent(ix + 1).into_iter().collect()
                };
                let refs = if ix % 7 == 0 {
                    vec![CommitRef {
                        kind: CommitRefKind::Tag,
                        name: format!("v{ix}"),
                        full_name: format!("refs/tags/v{ix}"),
                        current: false,
                    }]
                } else {
                    vec![]
                };
                Commit {
                    sha: sha(ix),
                    parents,
                    author_name: format!("Author {}", ix % 13),
                    author_email: format!("author{}@example.com", ix % 13),
                    authored_at: 0,
                    committed_at: (count - ix) as i64 * 1000,
                    refs,
                    subject: format!("Commit {ix}"),
                    body: String::new(),
                }
            })
            .collect()
    }

    fn stash(name: &str, base: &str, created_at: i64) -> Stash {
        Stash {
            sha: format!("{name:f>40}"),
            base: base.into(),
            message: format!("WIP {name}"),
            created_at,
        }
    }

    /// The first `loaded` of `commits`, as the history loads them: 200, then a thousand at a time.
    fn loaded(commits: &[Commit], loaded: usize) -> LogCommits {
        let first = loaded.min(200);
        let log = LogCommits::from(commits[..first].to_vec());
        more(&log, commits, loaded)
    }

    /// `log`, with more of `commits` loaded after it, up to `loaded`.
    fn more(log: &LogCommits, commits: &[Commit], loaded: usize) -> LogCommits {
        let mut log = log.clone();
        for page in commits[log.len()..loaded].chunks(1000) {
            log = log.followed_by(page.to_vec());
        }
        log
    }

    fn inputs(commits: LogCommits, stashes: &[Stash], has_changes: bool) -> LayoutInputs {
        LayoutInputs {
            commits: Some(commits),
            stashes: stashes.into(),
            has_changes,
            head: Some(sha(0)),
            remotes: vec!["origin".into()],
            ..LayoutInputs::none(repository_id())
        }
    }

    /// The rows as laid out whole, as `git/rows.ts` does.
    fn whole(inputs: &LayoutInputs) -> Vec<HistoryRow> {
        let commits: Vec<Commit> = inputs
            .commits
            .iter()
            .flat_map(LogCommits::iter)
            .cloned()
            .collect();
        to_history_rows(
            &inputs.repository_id,
            &with_stashes(&commits, &inputs.stashes),
            inputs.has_changes,
            inputs.head.as_deref(),
            &checkouts(&inputs.worktrees),
            &inputs.remotes,
        )
    }

    /// Lays `inputs` out after `previous`, checking the rows are those laid out whole, each in
    /// its place.
    fn check(previous: &HistoryLayout, inputs: LayoutInputs) -> HistoryLayout {
        let layout = HistoryLayout::lay_out(previous, inputs.clone());
        let rows: Vec<HistoryRow> = layout.iter().cloned().collect();
        assert_eq!(rows, whole(&inputs));
        assert_eq!(layout.len(), rows.len());
        for (ix, row) in rows.iter().enumerate() {
            assert_eq!(layout.get(ix), Some(row));
            assert_eq!(layout.position(&row.id()), Some(ix), "{:?}", row.id());
        }
        assert_eq!(layout.get(rows.len()), None);
        let widest = rows
            .iter()
            .filter_map(HistoryRow::graph)
            .map(|graph| graph.width)
            .max()
            .unwrap_or(1);
        assert_eq!(layout.lanes(), widest.max(1));
        layout
    }

    /// How many of `layout`'s parts are `previous`'s, kept.
    fn kept(previous: &HistoryLayout, layout: &HistoryLayout) -> usize {
        layout
            .parts
            .iter()
            .zip(&previous.parts)
            .filter(|(a, b)| Arc::ptr_eq(a, b))
            .count()
    }

    #[test]
    fn lays_out_a_page_added_as_it_lays_out_the_whole_history() {
        let commits = history(3500);
        let stashes = [
            // On a commit of the first page.
            stash("a", &sha(3), 0),
            // On one of a later page: by its date until that's loaded.
            stash("b", &sha(2600), 3_400_500),
            // Older than every commit loaded, until the last page is.
            stash("c", "0", 1500),
        ];
        let mut log = loaded(&commits, 200);
        let mut layout = check(
            &HistoryLayout::empty(repository_id()),
            inputs(log.clone(), &stashes, true),
        );
        for count in [1200, 2200, 3200, 3500] {
            log = more(&log, &commits, count);
            let next = check(&layout, inputs(log.clone(), &stashes, true));
            // Only the part added is laid out, and those the stashes moved out of or into.
            assert!(
                kept(&layout, &next) >= layout.parts.len().saturating_sub(2),
                "{count}"
            );
            assert_eq!(next.parts.len(), layout.parts.len() + 1);
            layout = next;
        }
    }

    #[test]
    fn lays_out_again_only_the_rows_the_uncommitted_changes_reach() {
        let log = loaded(&history(5200), 5200);
        let without = check(
            &HistoryLayout::empty(repository_id()),
            inputs(log.clone(), &[], false),
        );
        let with = check(&without, inputs(log.clone(), &[], true));
        // HEAD's the first commit: the graph's back as it was below it.
        assert_eq!(kept(&without, &with), with.parts.len() - 1);
        let without_again = check(&with, inputs(log.clone(), &[], false));
        assert_eq!(kept(&with, &without_again), with.parts.len() - 1);

        // Based further down, the line to HEAD passes by the rows above it.
        let deep = LayoutInputs {
            head: Some(sha(2500)),
            ..inputs(log, &[], true)
        };
        check(&with, deep);
    }

    #[test]
    fn lays_out_the_labels_again_once_the_worktrees_or_the_remotes_change() {
        let log = loaded(&history(1500), 1500);
        let first = check(
            &HistoryLayout::empty(repository_id()),
            inputs(log.clone(), &[], false),
        );
        let remotes = LayoutInputs {
            remotes: vec!["upstream".into()],
            ..inputs(log, &[], false)
        };
        let relabelled = check(&first, remotes);
        assert_eq!(kept(&first, &relabelled), 0);
    }

    #[test]
    fn has_no_rows_until_the_log_is_loaded() {
        let layout = HistoryLayout::lay_out(
            &HistoryLayout::empty(repository_id()),
            LayoutInputs {
                has_changes: true,
                ..LayoutInputs::none(repository_id())
            },
        );
        assert!(layout.is_empty());
        // An empty log has only the uncommitted changes.
        check(&layout, inputs(LogCommits::default(), &[], true));
    }

    /// Lays out 100,000 commits as they load, a page at a time, and once more with the
    /// uncommitted changes, as the table did before (the whole history each time, the commits
    /// copied into one list) and as it does now. Prints how long that took; run it with
    /// `cargo test -p app history_layout -- --ignored --nocapture`, best with the app built
    /// optimized.
    #[test]
    #[ignore = "a measurement"]
    fn measures_laying_out_100000_commits_as_they_load() {
        let commits = history(100_000);
        let report = |what: &str, times: &[Duration]| {
            let total: Duration = times.iter().sum();
            let slowest = times.iter().max().copied().unwrap_or_default();
            eprintln!("{what}: {total:.2?} in all, the slowest {slowest:.2?}");
        };

        // Before: each page copied the log into one list, then laid it all out, and mapped each
        // row's place.
        let mut times = Vec::new();
        let mut log: Arc<[Commit]> = commits[..200].to_vec().into();
        let whole_layout = |log: &[Commit], has_changes: bool| {
            let rows = to_history_rows(
                &repository_id(),
                &with_stashes(log, &[]),
                has_changes,
                Some(&sha(0)),
                &checkouts(&[]),
                &["origin"],
            );
            let places: HashMap<RowId, usize> = rows
                .iter()
                .enumerate()
                .map(|(ix, row)| (row.id(), ix))
                .collect();
            (rows, places)
        };
        for page in commits[200..].chunks(1000) {
            let started = Instant::now();
            let listed: std::collections::HashSet<&str> =
                log.iter().map(|commit| commit.sha.as_str()).collect();
            let more = page
                .iter()
                .filter(|commit| !listed.contains(commit.sha.as_str()));
            let next: Arc<[Commit]> = log.iter().cloned().chain(more.cloned()).collect();
            drop(listed);
            log = next;
            drop(whole_layout(&log, false));
            times.push(started.elapsed());
        }
        report("before, a page at a time", &times);
        let started = Instant::now();
        drop(whole_layout(&log, true));
        report("before, with the uncommitted changes", &[started.elapsed()]);

        // Now.
        let mut times = Vec::new();
        let mut log = LogCommits::from(commits[..200].to_vec());
        let mut layout = HistoryLayout::empty(repository_id());
        for page in commits[200..].chunks(1000) {
            let started = Instant::now();
            log = log.followed_by(page.to_vec());
            layout = HistoryLayout::lay_out(&layout, inputs(log.clone(), &[], false));
            times.push(started.elapsed());
        }
        report("now, a page at a time", &times);
        let started = Instant::now();
        let with = HistoryLayout::lay_out(&layout, inputs(log.clone(), &[], true));
        report("now, with the uncommitted changes", &[started.elapsed()]);
        assert_eq!(with.len(), 100_001);
    }
}
