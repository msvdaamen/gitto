//! The history: the log, loaded a page at a time, with what's shown among it, and a single
//! commit. Ported from `git/queries/history.ts`.
//!
//! The history table's rows are laid out from what [`History`] loads with `git/rows.ts`'s
//! `withStashes`, `historyGraph` and `toHistoryRows`, and `ref-labels.ts`'s `checkouts`: pure
//! logic, which the `gitto-ui` crate ports, so the table lays them out (see `crate::git`).

use std::cell::Cell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

use futures::FutureExt as _;
use futures::future::BoxFuture;
use gitto_git::features::history::commands::{Page, get_commit, get_log};
use gitto_git::features::history::schema::Commit;
use gitto_git::features::stash::schema::Stash;
use gitto_git::features::status::schema::{StatusSummary, Uncommitted};
use gitto_git::features::worktree::schema::Worktree;
use gitto_git::input::{RepositoryId, Sha};
use gpui_kit::{App, AppContext as _, Context, Entity, Subscription, Task};

use super::call::{git, loads, parse};
use super::client::{QueryClient, QueryError, QueryOptions, StaleTime};
use super::keys;
use super::mutation::Mutation;
use super::opaque::Opaque;
use super::query::Query;
use super::stash::stashes_query;
use super::status::{head_sha, status_query};
use super::worktree::worktrees_query;
use crate::git::status::has_uncommitted_changes;

/// How many commits the history loads first: a few screenfuls, so it shows quickly.
pub const FIRST_PAGE: u32 = 200;

/// How many more it loads at a time as the end of what's loaded comes into view.
pub const NEXT_PAGE: u32 = 1000;

/// The most commits the history reloads as the repository changes, which is the most the log
/// hands over at once (see `history::commands::MAX_PAGE`). Past that, it starts over from the
/// first page: the graph has to be laid out again, and the commits sent over, for every one of
/// them.
pub const RELOAD_LIMIT: usize = 10_000;

/// A repository's log, as [`log_options`] loads it: its newest commits, and those loaded after
/// them. Shared: a reload equal to what was there keeps it, so one that brings nothing new (the
/// refs changed elsewhere, say) doesn't lay out and render the history again (TS's
/// `reconcile: "sha"`).
#[derive(Debug, Clone, PartialEq)]
pub struct Log {
    /// The repository it's of, so every row knows where its commit lives.
    pub repository_id: String,
    pub commits: Arc<[Commit]>,
    /// Whether that's all of the history: the last page loaded was short of its limit.
    pub complete: bool,
}

/// Reads a page of the log of a repository: `limit` commits after skipping `skip`
/// (`rpc.git.history.log`): the app's from git, tests' made up.
pub type GetLog =
    Rc<dyn Fn(RepositoryId, u32, u32) -> BoxFuture<'static, Result<Vec<Commit>, QueryError>>>;

/// Git's `history.log` (see [`GetLog`]).
pub fn git_log(cx: &App) -> GetLog {
    git_log_from(git(cx))
}

/// `history.log`, run by `git`.
pub fn git_log_from(git: gitto_git::GitApi) -> GetLog {
    Rc::new(move |id, skip, limit| {
        git.call("history.log", &id, move |repo| async move {
            get_log(&repo, Page::new(limit, skip)?).await
        })
        .map(|result| result.map_err(Into::into))
        .boxed()
    })
}

/// How many commits a reload of the log asks for, when `loaded` are: as many as were loaded, so
/// the history keeps its place, but at least the first page, and only that past `RELOAD_LIMIT`.
pub fn reload_limit(loaded: usize) -> u32 {
    if loaded > RELOAD_LIMIT {
        FIRST_PAGE
    } else {
        FIRST_PAGE.max(loaded as u32)
    }
}

/// A repository's log (see [`Log`]): the first `FIRST_PAGE` commits, then `NEXT_PAGE` more each
/// time [`load_more`] runs. A reload, as the repository changes, reloads as many as were loaded in
/// one go, so the history keeps its place; git walks the whole history to sort it anyway, so that
/// takes little longer than the first page (220ms for 6,200 commits of vscode, against 150ms for
/// 200). Past `RELOAD_LIMIT`, it's back to the first page.
pub fn log_options(id: &RepositoryId, get: GetLog) -> QueryOptions<Log> {
    let repository = id.clone();
    QueryOptions::new(keys::log(id), move |previous: Option<&Log>, _| {
        let loaded = previous.map_or(0, |log| log.commits.len());
        let limit = reload_limit(loaded);
        let page = get(repository.clone(), 0, limit);
        let repository_id = repository.to_string();
        async move {
            let commits = page.await?;
            Ok(Log {
                repository_id,
                complete: commits.len() < limit as usize,
                commits: commits.into(),
            })
        }
        .boxed()
    })
}

/// Loads `NEXT_PAGE` more commits of the repository `id`'s log, after those loaded, unless it's
/// all loaded (`useLog`'s `more`). What it loads is dropped if the log reloaded meanwhile: it may
/// not follow on from what was reloaded.
pub fn load_more(id: RepositoryId, get: &GetLog, cx: &mut App) -> Task<Result<(), QueryError>> {
    let key = keys::log(&id);
    let Some(log) = QueryClient::get_data::<Log>(&key, cx) else {
        return Task::ready(Ok(()));
    };
    if log.complete {
        return Task::ready(Ok(()));
    }
    let updates = QueryClient::updates(&key, cx);
    let page = get(id, log.commits.len() as u32, NEXT_PAGE);
    cx.spawn(async move |cx| {
        let commits = cx.background_spawn(page).await?;
        cx.update(|cx| {
            // Reloaded meanwhile: this one may not follow on from what was reloaded.
            if QueryClient::updates(&key, cx) != updates {
                return;
            }
            let complete = commits.len() < NEXT_PAGE as usize;
            QueryClient::set_data(&key, followed_by(&log, commits, complete), cx);
        });
        Ok(())
    })
}

/// `log` with `commits` after it. Each commit only once: the history may have moved since the
/// commits before were loaded, until the reload that follows the move.
fn followed_by(log: &Log, commits: Vec<Commit>, complete: bool) -> Log {
    let listed: HashSet<&str> = log
        .commits
        .iter()
        .map(|commit| commit.sha.as_str())
        .collect();
    let more = commits
        .into_iter()
        .filter(|commit| !listed.contains(commit.sha.as_str()));
    Log {
        repository_id: log.repository_id.clone(),
        commits: log.commits.iter().cloned().chain(more).collect(),
        complete,
    }
}

/// A repository's log as a view shows it (`useLog`): the query, and loading more of it. A view
/// keeps one, made in its constructor, and makes another for another repository.
pub struct LogQuery {
    id: RepositoryId,
    query: Query<Log>,
    more: Entity<Mutation<RepositoryId>>,
    _subscriptions: [Subscription; 2],
}

impl LogQuery {
    pub fn new<V: 'static>(id: &RepositoryId, cx: &mut Context<V>) -> Self {
        LogQuery::with_get(id, git_log(cx), cx)
    }

    /// The log, read by `get`: e.g. a test's.
    pub fn with_get<V: 'static>(id: &RepositoryId, get: GetLog, cx: &mut Context<V>) -> Self {
        let query = Query::new(log_options(id, Rc::clone(&get)), cx);
        let more = Mutation::new(move |id, cx: &mut App| load_more(id, &get, cx), cx);
        // Whether loading more failed, or is still under way, is of the log as it was: not of the
        // one reloaded since (`useResetOn`).
        let reset = {
            let more = more.clone();
            let last = Cell::new(query.updates());
            cx.observe(query.entry(), move |_, entry, cx| {
                let updates = entry.read(cx).updates();
                if last.replace(updates) != updates {
                    more.update(cx, |more, cx| more.reset(cx));
                }
            })
        };
        let render = cx.observe(&more, |_, _, cx| cx.notify());
        LogQuery {
            id: id.clone(),
            query,
            more,
            _subscriptions: [reset, render],
        }
    }

    pub fn query(&self) -> &Query<Log> {
        &self.query
    }

    /// Loading more of the log: whether it's under way, and why it last failed.
    pub fn more(&self) -> &Entity<Mutation<RepositoryId>> {
        &self.more
    }

    /// Whether there's more of the log to load, once it's loaded.
    pub fn has_more(&self) -> bool {
        self.query.data().is_some_and(|log| !log.complete)
    }

    /// Loads more of the log, unless it's loading, or reloading, which would leave that unused.
    pub fn load_more(&self, cx: &mut App) {
        if self.has_more() && !self.query.is_fetching() && !self.more.read(cx).is_pending() {
            let id = self.id.clone();
            self.more.update(cx, |more, cx| {
                more.run(id, cx);
            });
        }
    }
}

/// What the history table shows (`useHistory`): the log, and what's shown among it. The rows are
/// laid out from these (see the module's doc): the log's commits with the stashes among them, a
/// row of the uncommitted changes if there are any, and the other worktrees' checkouts on their
/// labels.
pub struct History {
    log: LogQuery,
    status: Query<Opaque<Uncommitted>, StatusSummary>,
    stashes: Query<Arc<[Stash]>>,
    worktrees: Query<Arc<[Worktree]>>,
}

impl History {
    pub fn new<V: 'static>(id: &RepositoryId, cx: &mut Context<V>) -> Self {
        History {
            log: LogQuery::new(id, cx),
            status: status_query(id, cx),
            stashes: stashes_query(id, cx),
            worktrees: worktrees_query(id, cx),
        }
    }

    pub fn log(&self) -> &LogQuery {
        &self.log
    }

    /// The log's commits, once it's loaded.
    pub fn commits(&self) -> Option<Log> {
        self.log.query().data()
    }

    pub fn status(&self) -> &Query<Opaque<Uncommitted>, StatusSummary> {
        &self.status
    }

    pub fn stashes(&self) -> &Query<Arc<[Stash]>> {
        &self.stashes
    }

    pub fn worktrees(&self) -> &Query<Arc<[Worktree]>> {
        &self.worktrees
    }

    /// Whether there are uncommitted changes, for their row. The layout only depends on the log,
    /// the stashes, this and HEAD; not on the rest of the status, which reloads whenever a file
    /// changes.
    pub fn has_changes(&self) -> bool {
        has_uncommitted_changes(self.status.data().as_ref())
    }

    /// The commit HEAD points at; `None` before the first commit, or until the status loads.
    pub fn head(&self) -> Option<String> {
        self.status
            .data()
            .and_then(|status| head_sha(&status.head).map(str::to_owned))
    }

    /// The stashes to show among the log: none until they've loaded, rather than holding up the
    /// history for them, and none if they couldn't be: the Pop button says why.
    pub fn shown_stashes(&self) -> Arc<[Stash]> {
        loaded(&self.stashes)
    }

    /// The worktrees whose checkouts the labels show: none until they've loaded, and if they
    /// couldn't be, as with the stashes.
    pub fn shown_worktrees(&self) -> Arc<[Worktree]> {
        loaded(&self.worktrees)
    }

    /// Whether there's more of the log to load, once it's loaded.
    pub fn has_more(&self) -> bool {
        self.log.has_more()
    }

    /// Loads more of the log (see `LogQuery::load_more`).
    pub fn load_more(&self, cx: &mut App) {
        self.log.load_more(cx)
    }
}

fn loaded<T: PartialEq + Send + Sync + 'static>(query: &Query<Arc<[T]>>) -> Arc<[T]> {
    match query.data() {
        Some(data) if query.is_success() => data,
        _ => Arc::from(Vec::new()),
    }
}

/// A single commit, e.g. for the details of the selected one (`useCommitDetails`). Taken from the
/// log when it's in it, as the selected one is: the log has all of a commit, so git isn't asked
/// for it again. Loaded on its own otherwise. Shown with `Query::set_options` in place of the one
/// before, which stays on show while it loads, instead of nothing.
pub fn commit_details_options(id: &RepositoryId, sha: &str, cx: &App) -> QueryOptions<Commit> {
    let initial = commit_in_log(id, sha);
    let sha = sha.to_owned();
    QueryOptions::new(
        keys::commit(id, &sha),
        loads(cx, "history.commit", id, move |repo| {
            let sha = sha.clone();
            async move { get_commit(&repo, &parse::<Sha>(sha)?).await }
        }),
    )
    .initial_data(initial)
    .stale_time(StaleTime::Infinite)
    .keep_previous_data()
}

/// The commit `sha` as the repository `id`'s cached log has it, if it's in it: a commit's details'
/// initial data (see [`commit_details_options`]).
fn commit_in_log(id: &RepositoryId, sha: &str) -> impl Fn(&App) -> Option<Commit> + 'static {
    let log = keys::log(id);
    let sha = sha.to_owned();
    move |cx| {
        QueryClient::get_data::<Log>(&log, cx)?
            .commits
            .iter()
            .find(|commit| commit.sha == sha)
            .cloned()
    }
}

/// A commit's details' query (see [`commit_details_options`]).
pub fn commit_details_query<V: 'static>(
    id: &RepositoryId,
    sha: &str,
    cx: &mut Context<V>,
) -> Query<Commit> {
    Query::new(commit_details_options(id, sha, cx), cx)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use futures::channel::oneshot;
    use gpui_kit::TestAppContext;

    use super::*;

    fn repository_id() -> RepositoryId {
        RepositoryId::parse("01890a5d-ac96-774b-bcce-b302099a8057").unwrap()
    }

    fn commit(sha: &str) -> Commit {
        Commit {
            sha: sha.into(),
            parents: vec![],
            author_name: "Ada".into(),
            author_email: "ada@example.com".into(),
            authored_at: 0,
            committed_at: 0,
            refs: vec![],
            subject: sha.into(),
            body: String::new(),
        }
    }

    fn commits(range: std::ops::Range<usize>) -> Vec<Commit> {
        range.map(|n| commit(&format!("{n:040x}"))).collect()
    }

    /// The pages asked for, and what answers each, when the test says.
    type Asked = Rc<RefCell<Vec<(u32, u32, oneshot::Sender<Vec<Commit>>)>>>;

    fn get_log() -> (GetLog, Asked) {
        let asked: Asked = Rc::new(RefCell::new(Vec::new()));
        let get: GetLog = {
            let asked = Rc::clone(&asked);
            Rc::new(move |_, skip, limit| {
                let (answer, answered) = oneshot::channel();
                asked.borrow_mut().push((skip, limit, answer));
                async move { Ok(answered.await.unwrap()) }.boxed()
            })
        };
        (get, asked)
    }

    fn answer(asked: &Asked, commits: Vec<Commit>) -> (u32, u32) {
        let (skip, limit, answer) = asked.borrow_mut().remove(0);
        answer.send(commits).unwrap();
        (skip, limit)
    }

    struct Probe {
        log: LogQuery,
    }

    fn probe(get: GetLog, cx: &mut TestAppContext) -> Entity<Probe> {
        cx.update(crate::data::init);
        cx.new(|cx| Probe {
            log: LogQuery::with_get(&repository_id(), get, cx),
        })
    }

    /// Runs git in `cwd`, as a committer.
    fn git(cwd: &std::path::Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .args(["-c", "user.name=Ada", "-c", "user.email=ada@example.com"])
            .args([
                "-c",
                "commit.gpgsign=false",
                "-c",
                "init.defaultBranch=main",
            ])
            .args(args)
            .current_dir(cwd)
            .output()
            .expect("git runs");
        assert!(status.status.success(), "git {args:?}: {status:?}");
    }

    #[test]
    fn reads_a_page_of_a_real_log() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_owned();
        git(&path, &["init", "-q"]);
        for message in ["one", "two", "three"] {
            git(&path, &["commit", "-q", "--allow-empty", "-m", message]);
        }
        let get = git_log_from(gitto_git::GitApi::new(move |_: &str| Some(path.clone())));
        let page = smol::block_on(get(repository_id(), 1, 1)).unwrap();
        assert_eq!(page.len(), 1);
        assert_eq!(page[0].subject, "two");
        // A page the contract refuses fails as the API says.
        let refused = smol::block_on(get(repository_id(), 0, 0)).unwrap_err();
        assert_eq!(refused.message(), "Input validation failed");
    }

    #[test]
    fn reloads_as_many_as_were_loaded_up_to_the_limit() {
        assert_eq!(reload_limit(0), FIRST_PAGE);
        assert_eq!(reload_limit(150), FIRST_PAGE);
        assert_eq!(reload_limit(1200), 1200);
        assert_eq!(reload_limit(RELOAD_LIMIT), RELOAD_LIMIT as u32);
        assert_eq!(reload_limit(RELOAD_LIMIT + 1), FIRST_PAGE);
    }

    #[gpui_kit::test]
    fn loads_more_and_reloads_as_many(cx: &mut TestAppContext) {
        let (get, asked) = get_log();
        let probe = probe(get, cx);
        cx.run_until_parked();
        assert_eq!(answer(&asked, commits(0..200)), (0, FIRST_PAGE));
        cx.run_until_parked();
        assert!(probe.read_with(cx, |probe, _| probe.log.has_more()));

        probe.update(cx, |probe, cx| probe.log.load_more(cx));
        cx.run_until_parked();
        // Each commit only once: the history moved by one since.
        assert_eq!(answer(&asked, commits(199..1199)), (200, NEXT_PAGE));
        cx.run_until_parked();
        let log = probe.read_with(cx, |probe, _| probe.log.query().data().unwrap());
        assert_eq!(log.commits.len(), 1199);
        assert!(!log.complete);
        assert!(!probe.read_with(cx, |probe, cx| probe.log.more().read(cx).is_pending()));

        let reload = probe.update(cx, |probe, cx| probe.log.query().refetch(cx));
        cx.run_until_parked();
        assert_eq!(answer(&asked, commits(0..10)), (0, 1199));
        cx.run_until_parked();
        reload.now_or_never().unwrap();
        let log = probe.read_with(cx, |probe, _| probe.log.query().data().unwrap());
        assert!(log.complete);
        assert!(!probe.read_with(cx, |probe, _| probe.log.has_more()));
    }

    #[gpui_kit::test]
    fn drops_a_page_loaded_while_the_log_reloaded(cx: &mut TestAppContext) {
        let (get, asked) = get_log();
        let probe = probe(get, cx);
        cx.run_until_parked();
        answer(&asked, commits(0..200));
        cx.run_until_parked();

        probe.update(cx, |probe, cx| probe.log.load_more(cx));
        cx.run_until_parked();
        let reload = probe.update(cx, |probe, cx| probe.log.query().refetch(cx));
        cx.run_until_parked();
        // The reload's answer comes first, then the page's.
        let (_, _, page) = asked.borrow_mut().remove(0);
        assert_eq!(answer(&asked, commits(0..200)), (0, FIRST_PAGE));
        cx.run_until_parked();
        reload.now_or_never().unwrap();
        page.send(commits(200..1200)).unwrap();
        cx.run_until_parked();

        let log = probe.read_with(cx, |probe, _| probe.log.query().data().unwrap());
        assert_eq!(log.commits.len(), 200);
        // Whether it was loading is of the log before the reload.
        assert!(!probe.read_with(cx, |probe, cx| probe.log.more().read(cx).is_pending()));
    }

    #[gpui_kit::test]
    fn takes_a_commit_from_the_log(cx: &mut TestAppContext) {
        cx.update(crate::data::init);
        let id = repository_id();
        let sha = format!("{:040x}", 1);
        let log = Log {
            repository_id: id.to_string(),
            commits: commits(0..3).into(),
            complete: true,
        };
        cx.update(|cx| QueryClient::set_data(&keys::log(&id), log, cx));
        let options = QueryOptions::<Commit>::new(keys::commit(&id, &sha), |_, _| {
            async { Err(QueryError::msg("Asked git.")) }.boxed()
        });
        let options = options
            .initial_data(commit_in_log(&id, &sha))
            .stale_time(StaleTime::Infinite);
        struct Details {
            query: Query<Commit>,
        }
        let details = cx.new(|cx| Details {
            query: Query::new(options, cx),
        });
        cx.run_until_parked();
        let shown = details.read_with(cx, |details, _| {
            (details.query.data(), details.query.is_error())
        });
        assert_eq!(shown.0.unwrap().sha, sha);
        assert!(!shown.1);
    }
}
