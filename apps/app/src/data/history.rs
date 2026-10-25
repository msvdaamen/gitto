//! The history: the log, loaded a page at a time, with what's shown among it, and a single
//! commit. Ported from `git/queries/history.ts`.
//!
//! The history table's rows are laid out from what [`History`] loads with `git/rows.ts`'s
//! `withStashes`, `historyGraph` and `toHistoryRows`, and `ref-labels.ts`'s `checkouts`: pure
//! logic, which the `gitto-ui` crate ports, so the table lays them out (see
//! `gitto_ui::git::rows`).

use std::collections::HashSet;
use std::sync::Arc;

use gitto_git::features::history::schema::Commit;
use gitto_git::features::stash::schema::Stash;
use gitto_git::features::status::schema::StatusSummary;
use gitto_git::features::worktree::schema::Worktree;

use super::loaded::Loaded;
use super::repository::{Part, Repository};
use super::status::head_sha;
use gitto_ui::git::status::has_uncommitted_changes;

/// How many commits the history loads first: a few screenfuls, so it shows quickly.
pub const FIRST_PAGE: u32 = 200;

/// How many more it loads at a time as the end of what's loaded comes into view.
pub const NEXT_PAGE: u32 = 1000;

/// The most commits the history reloads as the repository changes, which is the most the log
/// hands over at once (see `history::commands::MAX_PAGE`). Past that, it starts over from the
/// first page: the graph has to be laid out again, and the commits sent over, for every one of
/// them.
pub const RELOAD_LIMIT: usize = 10_000;

/// A repository's log, as `Repository::log` has it: its newest commits, and those loaded after
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

/// How many commits a reload of the log asks for, when `loaded` are: as many as were loaded, so
/// the history keeps its place, but at least the first page, and only that past `RELOAD_LIMIT`.
/// Git walks the whole history to sort it anyway, so that takes little longer than the first page
/// (220ms for 6,200 commits of vscode, against 150ms for 200).
pub fn reload_limit(loaded: usize) -> u32 {
    if loaded > RELOAD_LIMIT {
        FIRST_PAGE
    } else {
        FIRST_PAGE.max(loaded as u32)
    }
}

/// `log` with `commits` after it. Each commit only once: the history may have moved since the
/// commits before were loaded, until the reload that follows the move.
pub(crate) fn followed_by(log: &Log, commits: Vec<Commit>, complete: bool) -> Log {
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

/// What the history table shows of a repository (`useHistory`): the log, and what's shown among
/// it. The rows are laid out from these (see the module's doc): the log's commits with the stashes
/// among them, a row of the uncommitted changes if there are any, and the other worktrees'
/// checkouts on their labels.
pub struct History<'a>(pub &'a Repository);

impl History<'_> {
    /// What of a repository the history shows.
    pub const PARTS: &'static [Part] = &[
        Part::Log,
        Part::More,
        Part::Status,
        Part::Stashes,
        Part::Worktrees,
    ];

    /// The log's commits, once it's loaded.
    pub fn commits(&self) -> Option<&Log> {
        self.0.log().data()
    }

    /// Whether there are uncommitted changes, for their row. The layout only depends on the log,
    /// the stashes, this and HEAD; not on the rest of the status, which reloads whenever a file
    /// changes.
    pub fn has_changes(&self) -> bool {
        has_uncommitted_changes(self.summary().as_ref())
    }

    pub fn summary(&self) -> Option<StatusSummary> {
        self.0.status().data().map(|status| status.summary())
    }

    /// The commit HEAD points at; `None` before the first commit, or until the status loads.
    pub fn head(&self) -> Option<String> {
        let status = self.0.status().data()?;
        head_sha(&status.head).map(str::to_owned)
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
}

fn loaded<T>(loaded: &Loaded<Arc<[T]>>) -> Arc<[T]> {
    match loaded.data() {
        Some(data) if loaded.is_success() => Arc::clone(data),
        _ => Arc::from(Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use gitto_git::features::history::commands::Page;
    use gitto_git::input::RepositoryId;

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
        let api = gitto_git::GitApi::new(move |_: &str| Some(path.clone()));
        let repository = GitRepository::new(api, repository_id());
        let page = smol::block_on(repository.log(Page::new(1, 1).unwrap())).unwrap();
        assert_eq!(page.len(), 1);
        assert_eq!(page[0].subject, "two");
    }

    #[test]
    fn reloads_as_many_as_were_loaded_up_to_the_limit() {
        assert_eq!(reload_limit(0), FIRST_PAGE);
        assert_eq!(reload_limit(150), FIRST_PAGE);
        assert_eq!(reload_limit(1200), 1200);
        assert_eq!(reload_limit(RELOAD_LIMIT), RELOAD_LIMIT as u32);
        assert_eq!(reload_limit(RELOAD_LIMIT + 1), FIRST_PAGE);
    }
}
