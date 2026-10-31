//! The history: the log, loaded a page at a time, with what's shown among it, and a single
//! commit. Ported from `git/queries/history.ts`.
//!
//! The history table's rows are laid out from what [`History`] loads with `git/rows.ts`'s
//! `withStashes`, `historyGraph` and `toHistoryRows`, and `ref-labels.ts`'s `checkouts`: pure
//! logic, which the `gitto-ui-logic` crate ports, so the table lays them out (see
//! `gitto_ui_logic::git::rows`).

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use futures::FutureExt as _;
use gitto_git::features::history::schema::Commit;
use gitto_git::features::stash::schema::Stash;
use gitto_git::features::status::schema::StatusSummary;
use gitto_git::features::worktree::schema::Worktree;
use gitto_git::input::RepositoryId;

use super::git::{GitTask, RepositoryGit};
use super::loaded::Loaded;
use super::repository::{Part, Repository, log_commits};
use super::status::head_sha;
use gitto_ui_logic::git::status::has_uncommitted_changes;

/// How many commits the history loads first: a few screenfuls, so it shows quickly.
pub const FIRST_PAGE: u32 = 200;

/// How many more it loads at a time as the end of what's loaded comes into view.
pub const NEXT_PAGE: u32 = 1000;

/// The most commits the log hands over at once (see `history::commands::MAX_PAGE`): what the
/// history reloads at a time.
const MAX_PAGE: u32 = 10_000;

/// A repository's log, as `Repository::log` has it: its newest commits, and those loaded after
/// them. Shared: a reload equal to what was there keeps it, so one that brings nothing new (the
/// refs changed elsewhere, say) doesn't lay out and render the history again (TS's
/// `reconcile: "sha"`).
#[derive(Debug, Clone, PartialEq)]
pub struct Log {
    /// The repository it's of, so every row knows where its commit lives.
    pub repository_id: RepositoryId,
    pub commits: LogCommits,
    /// Whether that's all of the history: the last page loaded was short of its limit.
    pub complete: bool,
}

/// A log's commits, newest first, in the pages they were loaded in: loading more adds a page to
/// those before it, rather than copying them into one list with it, so the history lays out only
/// the page added.
#[derive(Clone, Default)]
pub struct LogCommits {
    pages: Vec<Arc<[Commit]>>,
    len: usize,
    /// The commits' SHAs, shared with the logs this one is followed by (see [`Listed`]).
    listed: Arc<Mutex<Listed>>,
}

/// The SHAs of a log's commits, for adding a page without listing them all again: by where each
/// is, among the first `of` commits. Shared by a log and those it's followed by, each a page longer
/// than the last, so each finds its own commits among the first of them.
#[derive(Default)]
struct Listed {
    at: HashMap<String, usize>,
    of: usize,
}

impl LogCommits {
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &Commit> {
        self.pages.iter().flat_map(|page| page.iter())
    }

    /// The pages, in the order they were loaded.
    pub fn pages(&self) -> &[Arc<[Commit]>] {
        &self.pages
    }

    /// Whether these are `other`'s pages, not only the same commits: telling them apart doesn't
    /// compare the commits.
    pub fn is(&self, other: &LogCommits) -> bool {
        self.len == other.len
            && self.pages.len() == other.pages.len()
            && self
                .pages
                .iter()
                .zip(&other.pages)
                .all(|(a, b)| Arc::ptr_eq(a, b))
    }

    /// These, then `commits`, but those of them that are listed already: the history may have
    /// moved since these were loaded, until the reload that follows the move.
    pub fn followed_by(&self, commits: Vec<Commit>) -> LogCommits {
        let mut listed = self.listed.lock().unwrap_or_else(PoisonError::into_inner);
        if listed.of != self.len {
            // Listed for a log this one was followed by already: listed again, for this one.
            *listed = Listed::of(self);
        }
        let more: Vec<Commit> = commits
            .into_iter()
            .filter(|commit| !listed.at.contains_key(&commit.sha))
            .collect();
        let mut followed = self.clone();
        if more.is_empty() {
            return followed;
        }
        for (ix, commit) in more.iter().enumerate() {
            listed.at.entry(commit.sha.clone()).or_insert(self.len + ix);
        }
        listed.of += more.len();
        followed.len += more.len();
        followed.pages.push(more.into());
        followed
    }
}

impl Listed {
    fn of(commits: &LogCommits) -> Listed {
        let mut at = HashMap::with_capacity(commits.len);
        for (ix, commit) in commits.iter().enumerate() {
            at.entry(commit.sha.clone()).or_insert(ix);
        }
        Listed {
            at,
            of: commits.len,
        }
    }
}

impl From<Vec<Commit>> for LogCommits {
    fn from(commits: Vec<Commit>) -> Self {
        LogCommits {
            len: commits.len(),
            pages: if commits.is_empty() {
                Vec::new()
            } else {
                vec![commits.into()]
            },
            listed: Arc::default(),
        }
    }
}

/// The same commits, however they were paged.
impl PartialEq for LogCommits {
    fn eq(&self, other: &Self) -> bool {
        self.is(other) || (self.len == other.len && self.iter().eq(other.iter()))
    }
}

impl fmt::Debug for LogCommits {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

/// How many commits a reload of the log asks for, when `loaded` are: as many as were loaded, so
/// the history keeps its place (the commit selected, and how far it's scrolled), but at least the
/// first page. Git walks the whole history to sort it anyway, so that takes little longer than the
/// first page (220ms for 6,200 commits of vscode, against 150ms for 200).
pub fn reload_limit(loaded: usize) -> u32 {
    FIRST_PAGE.max(u32::try_from(loaded).unwrap_or(u32::MAX))
}

/// Reads the log of the repository `id` again, as many commits as `loaded` (see
/// [`reload_limit`]): a page of up to [`MAX_PAGE`] at a time.
pub(crate) fn reload_log(
    git: &dyn RepositoryGit,
    id: &RepositoryId,
    loaded: usize,
) -> GitTask<Log> {
    let limit = reload_limit(loaded);
    let pages: Vec<(u32, GitTask<Vec<Commit>>)> = (0..limit)
        .step_by(MAX_PAGE as usize)
        .map(|skip| {
            let page = (limit - skip).min(MAX_PAGE);
            (page, log_commits(git, skip, page))
        })
        .collect();
    let mut log = Log {
        repository_id: id.clone(),
        commits: LogCommits::default(),
        complete: false,
    };
    async move {
        for (limit, page) in pages {
            let commits = page.await?;
            let complete = commits.len() < limit as usize;
            log = followed_by(&log, commits, complete);
            if complete {
                break;
            }
        }
        Ok(log)
    }
    .boxed()
}

/// `log` with `commits` after it. Each commit only once: the history may have moved since the
/// commits before were loaded, until the reload that follows the move.
pub(crate) fn followed_by(log: &Log, commits: Vec<Commit>, complete: bool) -> Log {
    Log {
        repository_id: log.repository_id.clone(),
        commits: log.commits.followed_by(commits),
        complete,
    }
}

/// What the history table shows of a repository (`useHistory`): the log, and what's shown among
/// it. The rows are laid out from these (see the module's doc): the log's commits with the stashes
/// among them, a row of the uncommitted changes if there are any, and on their labels the other
/// worktrees' checkouts and the remotes their remote branches are on.
pub struct History<'a>(pub &'a Repository);

impl History<'_> {
    /// What of a repository the history shows.
    pub const PARTS: &'static [Part] = &[
        Part::Log,
        Part::More,
        Part::Status,
        Part::Stashes,
        Part::Worktrees,
        Part::Remotes,
    ];

    /// The log's commits, once it's loaded.
    pub fn commits(&self) -> Option<&Log> {
        self.0.log().data()
    }

    /// Whether there are uncommitted changes, for their row. The layout only depends on the log,
    /// the stashes, this and HEAD; not on the rest of the status, which reloads whenever a file
    /// changes.
    pub fn has_changes(&self) -> bool {
        self.summary().is_some_and(has_uncommitted_changes)
    }

    pub fn summary(&self) -> Option<&StatusSummary> {
        self.0.status().data().map(|status| &status.summary)
    }

    /// The commit HEAD points at; `None` before the first commit, or until the status loads.
    pub fn head(&self) -> Option<String> {
        let status = self.0.status().data()?;
        head_sha(&status.summary.head).map(str::to_owned)
    }

    /// The stashes to show among the log: none until they've loaded, rather than holding up the
    /// history for them, and none if they couldn't be: the Pop button says why.
    pub fn shown_stashes(&self) -> Arc<[Stash]> {
        loaded(self.0.stashes())
    }

    /// The worktrees whose checkouts the labels show: none until they've loaded, and if they
    /// couldn't be, as with the stashes.
    pub fn shown_worktrees(&self) -> Arc<[Worktree]> {
        loaded(self.0.worktrees())
    }

    /// The remotes the labels' remote branches are put on: none until they've loaded, and none
    /// if they couldn't be, a remote branch then being taken to be on the part of its name before
    /// its first slash.
    pub fn shown_remotes(&self) -> Vec<String> {
        let loaded = self.0.remotes();
        match loaded.data() {
            Some(remotes) if loaded.is_success() => remotes.clone(),
            _ => Vec::new(),
        }
    }
}

fn loaded<T>(loaded: &Loaded<Arc<[T]>>) -> Arc<[T]> {
    match loaded.data() {
        Some(data) if loaded.is_success() => Arc::clone(data),
        _ => Arc::from(Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use gitto_git::Page;

    use super::*;
    use crate::data::git::{GitRepository, RepositoryGit};

    fn repository_id() -> RepositoryId {
        RepositoryId::parse("01890a5d-ac96-774b-bcce-b302099a8057").unwrap()
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
        let api = gitto_git::GitApi::new(move |_: &RepositoryId| Some(path.clone()));
        let repository = GitRepository::new(api, repository_id());
        let page = smol::block_on(repository.log(Page::new(1, 1).unwrap())).unwrap();
        assert_eq!(page.len(), 1);
        assert_eq!(page[0].subject, "two");
    }

    #[test]
    fn reloads_as_many_as_were_loaded() {
        assert_eq!(reload_limit(0), FIRST_PAGE);
        assert_eq!(reload_limit(150), FIRST_PAGE);
        assert_eq!(reload_limit(1200), 1200);
        assert_eq!(reload_limit(25_000), 25_000);
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

    fn shas(log: &Log) -> Vec<&str> {
        log.commits
            .iter()
            .map(|commit| commit.sha.as_str())
            .collect()
    }

    fn log(shas: &[&str]) -> Log {
        Log {
            repository_id: repository_id(),
            commits: shas
                .iter()
                .map(|sha| commit(sha))
                .collect::<Vec<_>>()
                .into(),
            complete: false,
        }
    }

    #[test]
    fn adds_a_page_after_the_commits_leaving_out_those_listed_already() {
        let first = log(&["c", "b"]);
        let second = followed_by(&first, vec![commit("b"), commit("a")], false);
        assert_eq!(shas(&second), ["c", "b", "a"]);
        // As a page of its own: the ones before it are shared, not copied.
        assert_eq!(second.commits.pages().len(), 2);
        assert!(Arc::ptr_eq(
            &first.commits.pages()[0],
            &second.commits.pages()[0]
        ));
        let third = followed_by(&second, vec![commit("a"), commit("z")], true);
        assert_eq!(shas(&third), ["c", "b", "a", "z"]);
        assert!(third.complete);
        // The log before it still has only its own.
        assert_eq!(shas(&first), ["c", "b"]);
    }

    #[test]
    fn adds_to_a_log_that_was_added_to_already_as_to_any_other() {
        let first = log(&["c", "b"]);
        let _second = followed_by(&first, vec![commit("a")], false);
        // `a` was listed for the other log, not this one.
        let again = followed_by(&first, vec![commit("a"), commit("b")], false);
        assert_eq!(shas(&again), ["c", "b", "a"]);
    }

    #[test]
    fn is_the_same_log_however_its_paged() {
        let paged = followed_by(&log(&["c", "b"]), vec![commit("a")], false);
        assert_eq!(paged, log(&["c", "b", "a"]));
        assert_ne!(paged, log(&["c", "a", "b"]));
    }
}
