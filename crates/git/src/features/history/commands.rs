//! Reading the history. Ported from `history/commands.ts`.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::parse::{LOG_FORMAT, parse_log};
use super::schema::Commit;
use crate::core::errors::{Error, GitError, Result};
use crate::core::repo::Repo;
use crate::input::Sha;

/// The most commits a page of the log has: enough for the history to reload all it has loaded at
/// once, up to a point. Git walks the whole history to sort it anyway, so it's little slower than
/// a page of a few hundred.
pub const MAX_PAGE: u32 = 10_000;

/// A page of the log: `limit` commits (1 to [`MAX_PAGE`], 200 by default), after skipping `skip`
/// (0 by default). Checked when it's made with `new`, or deserialized, as TS's contract did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawPage")]
pub struct Page {
    pub limit: u32,
    pub skip: u32,
}

impl Page {
    /// The page, if `limit` is 1 to [`MAX_PAGE`]; `Error::InvalidInput` otherwise.
    pub fn new(limit: u32, skip: u32) -> Result<Page> {
        if limit == 0 || limit > MAX_PAGE {
            return Err(Error::InvalidInput(format!(
                "A page of the log has 1 to {MAX_PAGE} commits. ({limit})"
            )));
        }
        Ok(Page { limit, skip })
    }
}

impl Default for Page {
    fn default() -> Self {
        Page {
            limit: 200,
            skip: 0,
        }
    }
}

/// A [`Page`] as it's sent, before it's checked.
#[derive(Deserialize)]
#[serde(default)]
struct RawPage {
    limit: u32,
    skip: u32,
}

impl Default for RawPage {
    fn default() -> Self {
        let Page { limit, skip } = Page::default();
        RawPage { limit, skip }
    }
}

impl TryFrom<RawPage> for Page {
    type Error = Error;

    fn try_from(raw: RawPage) -> Result<Page> {
        Page::new(raw.limit, raw.skip)
    }
}

/// A page of the history over every branch, remote and tag, most recently committed first across
/// all of them, like GitKraken; a commit still always comes after its children. Sorting by commit
/// date keeps rebased and amended work at the top, where sorting by author date would bury it.
///
/// Fails with `Error::LogOrderChanged` for a later page when the ones before it turn out to need
/// sorting, which they weren't when they were read: the log is then for reading again from its
/// first page.
pub async fn get_log(repo: &Repo, page: Page) -> Result<Vec<Commit>> {
    let has_head = repo.has_head().await;
    // Only refs a user would recognise; `--all` also picks up tool namespaces (e.g. refs/t3/*).
    // HEAD is included for detached checkouts, unless the branch has no commits yet.
    let mut revisions = vec!["--branches", "--remotes", "--tags"];
    if has_head {
        revisions.push("HEAD");
    }
    let log = |format: &str, sorted: bool, count: u32, skip: u32| {
        let mut args: Vec<String> = vec!["log".to_owned(), "-z".to_owned(), format.to_owned()];
        if sorted {
            args.push("--date-order".to_owned());
        }
        args.push("--decorate=full".to_owned());
        args.push(format!("--max-count={count}"));
        args.push(format!("--skip={skip}"));
        args.extend(revisions.iter().map(|arg| (*arg).to_owned()));
        args.push("--".to_owned());
        repo.read(args)
    };
    let read = |sorted: bool| {
        let output = log(LOG_FORMAT, sorted, page.limit, page.skip);
        async move { Ok(parse_log(&output.await?)) }
    };

    // To sort them with children first, git walks back to the oldest commit a ref points at first:
    // 0.5s on vscode, whose old remote branches go back years, 4.3s on linux, and seconds more
    // without a commit-graph, as in a fresh clone. Unsorted, it hands them over by date as it comes
    // across them, in 40-50ms on both, which is the same order unless a commit is dated before its
    // parent. So that's read first, and only sorted by git if a commit did end up below one of its
    // parents. That's remembered, so the unsorted log isn't read in vain every time after.
    //
    // Every page is read the same way, so each follows on from the ones before it: a later one
    // unsorted too, checked against the commits of those, which are read again for it (only their
    // SHAs). If the order was wrong across them, the pages already shown are too.
    let commits = if repo.sorts_log() {
        read(true).await?
    } else if page.skip == 0 {
        let commits = read(false).await?;
        if children_first(&HashSet::new(), &commits) {
            commits
        } else {
            repo.mark_log_needs_sorting();
            read(true).await?
        }
    } else {
        let before = log("--format=%H", false, page.skip, 0);
        let (before, commits) = futures::try_join!(before, read(false))?;
        let shown = before.split(['\0', '\n']).map(str::trim).collect();
        if !children_first(&shown, &commits) {
            repo.mark_log_needs_sorting();
            return Err(Error::LogOrderChanged);
        }
        commits
    };
    // For when it does need sorting. After the log, not alongside it: the first write can take
    // seconds, and would slow it down.
    // It starts now, and runs to its end without being awaited (TS's `void`).
    drop(repo.update_commit_graph());
    Ok(commits)
}

/// Whether every commit comes before its parents, as the history's graph needs them, after the
/// commits `shown` before them.
fn children_first(shown: &HashSet<&str>, commits: &[Commit]) -> bool {
    let mut listed = HashSet::new();
    for commit in commits {
        if commit
            .parents
            .iter()
            .any(|parent| listed.contains(parent.as_str()) || shown.contains(parent.as_str()))
        {
            return false;
        }
        listed.insert(commit.sha.as_str());
    }
    true
}

/// The commit `sha`, as the history lists it, e.g. the one selected in the history.
pub async fn get_commit(repo: &Repo, sha: &Sha) -> Result<Commit> {
    let args = [
        "log",
        "-z",
        LOG_FORMAT,
        "--decorate=full",
        "--max-count=1",
        "--end-of-options",
        sha.as_str(),
        "--",
    ];
    let commit = parse_log(&repo.read(args).await?).into_iter().next();
    // `sha` names a tree or blob, say, rather than a commit.
    commit.ok_or_else(|| {
        Error::Git(GitError::new(
            format!("{sha} is not a commit."),
            args.map(str::to_owned).to_vec(),
            Some(0),
            "",
        ))
    })
}

/// The first page of the log, for tests.
#[cfg(test)]
pub(crate) const PAGE: Page = Page { limit: 50, skip: 0 };

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use super::*;
    use crate::core::errors::GitErrorKind;
    use crate::core::repo::Observed;
    use crate::features::history::schema::{CommitRef, CommitRefKind};
    use crate::test_support::{Fixture, git, wait_for};

    /// Runs git in `cwd` with `GIT_COMMITTER_DATE` set to `date`; panics if it fails.
    fn git_dated(cwd: &Path, date: &str, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .env("GIT_COMMITTER_DATE", date)
            .status()
            .expect("git runs");
        assert!(status.success());
    }

    fn subjects(log: &[Commit]) -> Vec<&str> {
        log.iter().map(|commit| commit.subject.as_str()).collect()
    }

    /// `repo`, telling `logs` what each `log` it ran was given.
    fn counted(repo: &Repo) -> (Repo, Arc<Mutex<Vec<Vec<String>>>>) {
        let logs = Arc::new(Mutex::new(Vec::new()));
        let told = Arc::clone(&logs);
        let counted = repo.observed(move |observed| {
            if let Observed::Ran { args, .. } = observed
                && args.first().is_some_and(|arg| arg == "log")
            {
                told.lock().unwrap().push(args.clone());
            }
        });
        (counted, logs)
    }

    fn sorted(logs: &Mutex<Vec<Vec<String>>>) -> Vec<bool> {
        logs.lock()
            .unwrap()
            .iter()
            .map(|args| args.iter().any(|arg| arg == "--date-order"))
            .collect()
    }

    fn chain(path: &Path) -> PathBuf {
        path.join(".git/objects/info/commit-graphs/commit-graph-chain")
    }

    #[test]
    fn reads_the_log_of_branches_and_tags_but_not_other_refs() {
        let fx = Fixture::new();
        smol::block_on(async {
            let repo = fx.create_history_repo("history").await;
            let log = get_log(&repo, PAGE).await.unwrap();
            // Both sides of the merge have the same timestamp, so their order is up to git.
            let mut sorted = subjects(&log);
            sorted.sort();
            assert_eq!(
                sorted,
                ["Merge side", "first", "main commit", "side commit"]
            );
            assert_eq!(
                log[0].refs,
                [
                    CommitRef {
                        kind: CommitRefKind::Local,
                        name: "main".into(),
                        full_name: "refs/heads/main".into(),
                        current: true,
                    },
                    CommitRef {
                        kind: CommitRefKind::Tag,
                        name: "v1".into(),
                        full_name: "refs/tags/v1".into(),
                        current: false,
                    },
                ]
            );
            assert_eq!(log[0].author_name, "Test User");
            assert_eq!(log[0].parents.len(), 2);
        });
    }

    #[test]
    fn reads_a_single_commit() {
        let fx = Fixture::new();
        smol::block_on(async {
            let repo = fx.create_history_repo("history").await;
            let head = get_log(&repo, PAGE).await.unwrap().remove(0);
            let short = Sha::parse(&head.sha[..7]).unwrap();
            assert_eq!(get_commit(&repo, &short).await.unwrap(), head);

            let tree = git(repo.path(), ["rev-parse", "HEAD^{tree}"]);
            let error = get_commit(&repo, &Sha::parse(tree.clone()).unwrap())
                .await
                .unwrap_err();
            let git_error = error.as_git().expect("a GitError");
            assert_eq!(git_error.kind, GitErrorKind::Command);
            assert_eq!(error.to_string(), format!("{tree} is not a commit."));
        });
    }

    #[test]
    fn has_an_empty_log_without_commits() {
        let fx = Fixture::new();
        fx.create_repo("empty");
        smol::block_on(async {
            let repo = fx.repos.open("empty").await.unwrap();
            assert_eq!(get_log(&repo, PAGE).await.unwrap(), []);
        });
    }

    #[test]
    fn the_commit_graph_is_written_after_the_log_once_per_run() {
        let fx = Fixture::new();
        let path = fx.create_repo("graph");
        git(&path, ["commit", "-q", "--allow-empty", "-m", "first"]);
        smol::block_on(async {
            let repo = fx.repos.open("graph").await.unwrap();
            assert!(!chain(&path).exists());

            get_log(&repo, PAGE).await.unwrap();
            wait_for(Duration::from_secs(5), || chain(&path).exists()).await;
            let written = fs::read_to_string(chain(&path)).unwrap();

            // A commit since then isn't added until the app runs again.
            git(&path, ["commit", "-q", "--allow-empty", "-m", "second"]);
            get_log(&repo, PAGE).await.unwrap();
            // Resolves once the write is done, without writing again.
            repo.update_commit_graph().await;
            assert_eq!(fs::read_to_string(chain(&path)).unwrap(), written);
        });
    }

    #[test]
    fn the_commit_graph_isnt_needed_to_put_a_commit_dated_before_its_parent_above_it() {
        let fx = Fixture::new();
        let path = fx.create_repo("graph-backdated");
        git(&path, ["commit", "-q", "--allow-empty", "-m", "parent"]);
        git(&path, ["checkout", "-qb", "side"]);
        // As a rebase with --committer-date-is-author-date leaves them. Unsorted, git lists it
        // last.
        git_dated(
            &path,
            "2001-01-01T00:00:00Z",
            &["commit", "-q", "--allow-empty", "-m", "child"],
        );
        smol::block_on(async {
            let repo = fx.repos.open("graph-backdated").await.unwrap();

            assert_eq!(
                subjects(&get_log(&repo, PAGE).await.unwrap()),
                ["child", "parent"]
            );
            repo.update_commit_graph().await;
            assert!(chain(&path).exists());
            assert_eq!(
                subjects(&get_log(&repo, PAGE).await.unwrap()),
                ["child", "parent"]
            );
        });
    }

    #[test]
    fn the_commit_graph_doesnt_have_git_sort_a_log_thats_already_in_order() {
        let fx = Fixture::new();
        let path = fx.create_repo("graph-ordered");
        git(&path, ["commit", "-q", "--allow-empty", "-m", "first"]);
        smol::block_on(async {
            let repo = fx.repos.open("graph-ordered").await.unwrap();
            repo.update_commit_graph().await;
            assert!(chain(&path).exists());

            let (counted, logs) = counted(&repo);
            assert_eq!(get_log(&counted, PAGE).await.unwrap().len(), 1);
            let logs = logs.lock().unwrap();
            assert_eq!(logs.len(), 1);
            assert!(!logs[0].iter().any(|arg| arg == "--date-order"));
        });
    }

    #[test]
    fn the_commit_graph_has_git_sort_a_log_that_needed_it_straight_away_from_then_on() {
        let fx = Fixture::new();
        let path = fx.create_repo("graph-remembered");
        git(&path, ["commit", "-q", "--allow-empty", "-m", "parent"]);
        git_dated(
            &path,
            "2001-01-01T00:00:00Z",
            &["commit", "-q", "--allow-empty", "-m", "child"],
        );
        git(&path, ["branch", "old", "HEAD~1"]);
        smol::block_on(async {
            let repo = fx.repos.open("graph-remembered").await.unwrap();
            let (counted, logs) = counted(&repo);

            // Unsorted, then sorted.
            assert_eq!(
                subjects(&get_log(&counted, PAGE).await.unwrap()),
                ["child", "parent"]
            );
            assert_eq!(sorted(&logs), [false, true]);
            assert_eq!(
                subjects(&get_log(&counted, PAGE).await.unwrap()),
                ["child", "parent"]
            );
            assert_eq!(sorted(&logs), [false, true, true]);
        });
    }

    #[test]
    fn the_commit_graph_reads_a_later_page_as_the_first_was_read() {
        let fx = Fixture::new();
        let path = fx.create_repo("graph-paged");
        for subject in ["first", "second", "third"] {
            git(&path, ["commit", "-q", "--allow-empty", "-m", subject]);
        }
        smol::block_on(async {
            let repo = fx.repos.open("graph-paged").await.unwrap();
            let (counted, logs) = counted(&repo);

            let later = get_log(&counted, Page::new(2, 1).unwrap()).await.unwrap();
            assert_eq!(subjects(&later), ["second", "first"]);
            // Unsorted, as the first page was: the commits before it, and then it.
            assert_eq!(sorted(&logs), [false, false]);

            // Sorted, once the log needs it.
            repo.mark_log_needs_sorting();
            let later = get_log(&counted, Page::new(2, 1).unwrap()).await.unwrap();
            assert_eq!(subjects(&later), ["second", "first"]);
            assert_eq!(sorted(&logs), [false, false, true]);
        });
    }

    #[test]
    fn pages_follow_on_from_each_other_when_a_commit_is_dated_before_its_parent() {
        let fx = Fixture::new();
        let path = fx.create_repo("paged-backdated");
        let commit = |date, subject| {
            git_dated(
                &path,
                date,
                &["commit", "-q", "--allow-empty", "-m", subject],
            );
        };
        commit("2001-01-01T00:00:02Z", "X");
        git(&path, ["branch", "b"]);
        // Dated before its parent, X.
        commit("2001-01-01T00:00:01Z", "Y");
        git(&path, ["switch", "-q", "b"]);
        commit("2001-01-01T00:00:03Z", "Z");
        smol::block_on(async {
            let repo = fx.repos.open("paged-backdated").await.unwrap();
            let page = |skip| Page::new(2, skip).unwrap();

            // Unsorted, X comes before its child Y, which is past the first page.
            assert_eq!(
                subjects(&get_log(&repo, page(0)).await.unwrap()),
                ["Z", "X"]
            );
            let error = get_log(&repo, page(2)).await.unwrap_err();
            assert!(matches!(error, Error::LogOrderChanged), "{error:?}");
            assert_eq!(error.api().code(), "CONFLICT");

            // Read again from the first page, sorted: no commit repeated or left out.
            assert_eq!(
                subjects(&get_log(&repo, page(0)).await.unwrap()),
                ["Z", "Y"]
            );
            assert_eq!(subjects(&get_log(&repo, page(2)).await.unwrap()), ["X"]);
        });
    }

    #[test]
    fn the_commit_graph_doesnt_fail_the_log_when_it_cant_be_written() {
        let fx = Fixture::new();
        let path = fx.create_repo("graph-locked");
        git(&path, ["commit", "-q", "--allow-empty", "-m", "first"]);
        // Another git is writing it.
        fs::create_dir_all(path.join(".git/objects/info/commit-graphs")).unwrap();
        let mut lock = chain(&path).into_os_string();
        lock.push(".lock");
        fs::write(lock, "").unwrap();
        smol::block_on(async {
            let repo = fx.repos.open("graph-locked").await.unwrap();

            assert_eq!(get_log(&repo, PAGE).await.unwrap().len(), 1);
            repo.update_commit_graph().await;
            assert!(!chain(&path).exists());
        });
    }

    #[test]
    fn pages_are_checked_as_the_contract_did() {
        assert_eq!(
            Page::default(),
            Page {
                limit: 200,
                skip: 0
            }
        );
        assert!(Page::new(0, 0).is_err());
        assert!(Page::new(MAX_PAGE + 1, 0).is_err());
        assert_eq!(Page::new(MAX_PAGE, 3).unwrap().skip, 3);
        let page: Page = serde_json::from_str("{}").unwrap();
        assert_eq!(page, Page::default());
        let page: Page = serde_json::from_str(r#"{"limit":5,"skip":2}"#).unwrap();
        assert_eq!(page, Page { limit: 5, skip: 2 });
        assert!(serde_json::from_str::<Page>(r#"{"limit":0}"#).is_err());
        assert!(serde_json::from_str::<Page>(r#"{"skip":-1}"#).is_err());
    }
}
