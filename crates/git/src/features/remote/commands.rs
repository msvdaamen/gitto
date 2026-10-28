//! Listing remotes, fetching them, and pulling a branch's upstream into it.

use std::future::Future;
use std::path::{Path, PathBuf};

use futures::FutureExt;

use super::blockers::{Action, no_upstream};
use super::config::{
    RemoteConfig, is_false, makes_merge_commit, read_config, rebase_setting, rebases,
    remotes_to_fetch,
};
use super::progress::{in_progress, interruption, progress, why_detached};
use crate::core::errors::{Error, GitError, GitErrorKind, Result};
use crate::core::git_dirs::{GitDirs, git_dirs};
use crate::core::repo::{GitCommand, Repo, current_branch, ref_exists, resolve_ref, succeeds};
use crate::core::runner::{RunOptions, spawn_detached};
use crate::features::branch::in_use::branches_in_use;
use crate::input::BranchShortName;

/// The remotes, by name, as `run` lists them.
pub async fn list_remotes(run: &GitCommand) -> Result<Vec<String>> {
    Ok(run
        .run(["remote"])
        .await?
        .split('\n')
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect())
}

/// Fetches every remote, and drops remote-tracking branches whose branch was deleted on the
/// remote, so the sidebar and history show the remotes as they are.
pub async fn fetch_all(repo: &Repo) -> Result<()> {
    git_fetch(&repo.fetcher(), &["--all", "--prune", "--no-progress"], &[]).await
}

/// Fetches `remotes` and prunes, as `fetch_all` does, once a pull has merged: not waited for, so
/// a remote that's slow to answer doesn't hold up the pull, and one that can't be reached doesn't
/// fail it (that's for the Fetch button to explain). The watcher picks up the branches it changes.
fn fetch_after_pull(repo: &Repo, remotes: Vec<String>) {
    if remotes.is_empty() {
        return;
    }
    let mut args = vec!["--multiple", "--prune", "--no-progress", "--"];
    args.extend(remotes.iter().map(String::as_str));
    // Queued (as fetches are) before the pull returns, so a fetch started after it waits for this.
    let fetch = git_fetch(&repo.fetcher(), &args, &[]);
    spawn_detached(async move {
        let _ = fetch.await;
    });
}

/// `git fetch` with `args`, through `run` (`repo.fetcher()`, or the command of `repo.fetching`),
/// with the `-c` settings `settings`. The command is asked for now (taking its place in line, for
/// `fetcher()`), and run when the future is polled.
fn git_fetch(
    run: &GitCommand,
    args: &[&str],
    settings: &[&str],
) -> impl Future<Output = Result<()>> + Send + 'static + use<> {
    // Without the upkeep a fetch may start (gc, maintenance, the commit-graph): fetches run
    // alongside the queued writes, which that could get in the way of.
    let options = [
        "gc.auto=0",
        "maintenance.auto=false",
        "fetch.writeCommitGraph=false",
    ]
    .iter()
    .chain(settings)
    .fold(RunOptions::default(), |options, setting| {
        options.config(*setting)
    });
    let fetch = run.run_with(std::iter::once(&"fetch").chain(args), options);
    async move {
        fetch.await.map_err(Error::without_hints)?;
        Ok(())
    }
}

/// Fetches the current branch's upstream and merges it in, or rebases onto it if the user's
/// config says to, like `git pull`. Conflicts are left in the working tree, to resolve and commit.
/// Also fetches every remote and drops branches deleted on them, as the Fetch button does, so the
/// sidebar and history are up to date after a pull too: the upstream's remote first, and the
/// others once merged (see `fetch_after_pull`).
///
/// Done as a fetch and then a merge or rebase, rather than one `git pull`, so only the second
/// waits for (and holds up) the other writes to the repository: the fetch takes as long as the
/// network.
pub async fn pull(repo: &Repo) -> Result<()> {
    // Where git keeps FETCH_HEAD and the state of a rebase; asked for once, alongside the branch.
    // Taken only once the branch is read, which can fail first: that's the error to report.
    let dirs = git_dirs(repo).boxed().shared();
    let run = repo.git();
    let (read, dirs) = futures::join!(read_branch(&run, dirs.clone()), dirs);
    let (branch, upstream, settings) = read?;
    let git_dir = dirs?.git_dir;
    // Its merge commit is worded unless the pull will rebase, or only fast-forward, which makes
    // none (as the settings say before fetching).
    let word = makes_merge_commit(&settings, &branch);
    let fetched = fetch_upstream(repo, &git_dir, &upstream, word).await?;

    let integrated = integrate(repo, &git_dir, &branch, &upstream, fetched).await;
    // Also after a merge that stopped at conflicts, say: the remotes are fetched all the same.
    fetch_after_pull(repo, remotes_to_fetch(&settings, &upstream.remote));
    integrated
}

/// Pulls the local branch `branch` (by its short name): as `pull` does if it's checked out, and
/// otherwise by fetching its upstream and fast-forwarding it there, without checking it out, as
/// `git fetch origin main:main` would; whatever the config says about merging or rebasing, which
/// needs a working tree. Nothing changes if it has every commit of its upstream already. Fails,
/// changing nothing, if it has commits its upstream doesn't, which only merging or rebasing it,
/// checked out, brings together, or if it's checked out in another worktree, or being rebased or
/// bisected in one (`Error::PullBlocked`). Every remote is fetched too, as `pull` does.
pub async fn pull_branch(repo: &Repo, branch: &BranchShortName) -> Result<()> {
    let branch = branch.as_str();
    let run = repo.git();
    let (current, config) = futures::try_join!(current_branch(&run), read_config(&run))?;
    if current.as_deref() == Some(branch) {
        return pull(repo).await;
    }
    let upstream = read_upstream(&run, branch, &config).await?;
    let git_dir = git_dirs(repo).await?.git_dir;
    fetch_upstream(repo, &git_dir, &upstream, false).await?;

    let forwarded = fast_forward(repo, branch, &upstream).await;
    fetch_after_pull(repo, remotes_to_fetch(&config, &upstream.remote));
    forwarded
}

/// `reference` without its `refs/heads/` or `refs/remotes/` prefix.
fn short_name(reference: &str) -> &str {
    reference
        .strip_prefix("refs/heads/")
        .or_else(|| reference.strip_prefix("refs/remotes/"))
        .unwrap_or(reference)
}

/// Checks that `config` still has `branch` pull from `upstream`, as it did before fetching; fails
/// with `Error::RepositoryChanged` if it doesn't.
fn check_still_tracks(config: &RemoteConfig, branch: &str, upstream: &Upstream) -> Result<()> {
    if config.get(&format!("branch.{branch}.remote")) == Some(upstream.remote.as_str())
        && config.get(&format!("branch.{branch}.merge")) == Some(upstream.merge.as_str())
    {
        return Ok(());
    }
    Err(Error::RepositoryChanged(format!(
        "{branch}'s upstream changed while pulling it. Pull again."
    )))
}

/// Moves `branch`, which isn't checked out, to its upstream's commit, as one write; fails if
/// that isn't a fast-forward, the branch is in use (see `branches_in_use`), or the branch or its
/// upstream changed while fetching.
async fn fast_forward(repo: &Repo, branch: &str, upstream: &Upstream) -> Result<()> {
    let reference = format!("refs/heads/{branch}");
    let (branch, upstream) = (branch.to_owned(), upstream.clone());
    repo.exclusive(|run| async move {
        // As for `integrate`, read again now that no other write can change them.
        let (in_use, config, before) = futures::try_join!(
            branches_in_use(&run),
            read_config(&run),
            resolve_ref(&run, &reference),
        )?;
        if let Some(using) = in_use.get(&reference) {
            return Err(Error::PullBlocked(format!(
                "{branch} {}, so it wasn't pulled. {}",
                using.state, using.fix
            )));
        }
        let Some(before) = before else {
            return Err(Error::RepositoryChanged(format!(
                "{branch} was deleted while pulling it."
            )));
        };
        check_still_tracks(&config, &branch, &upstream)?;
        // A branch that isn't checked out has commits, so `for-each-ref` named its tracking
        // branch.
        let Some(tracking) = upstream.tracking.as_deref() else {
            return Err(Error::Bug(format!("{branch} has no tracking branch.")));
        };
        let Some(after) = resolve_ref(&run, tracking).await? else {
            return Err(Error::NoUpstream(no_upstream(&branch)));
        };
        // Up to date, or only ahead: nothing to bring in.
        if succeeds(&run, ["merge-base", "--is-ancestor", &after, &before]).await? {
            return Ok(());
        }
        if !succeeds(&run, ["merge-base", "--is-ancestor", &before, &after]).await? {
            return Err(Error::PullBlocked(format!(
                "{branch} and {} have diverged. Switch to {branch} to pull it.",
                short_name(tracking)
            )));
        }
        // Only if it's still where it was, as a write from a terminal could have moved it.
        run.run([
            "update-ref",
            "-m",
            "pull: Fast-forward",
            &reference,
            &after,
            &before,
        ])
        .await?;
        Ok(())
    })
    .await
}

/// Merges what `fetch_upstream` fetched into `branch`, or rebases onto it, as one write; fails if
/// that stopped partway, or the branch or its upstream changed while fetching.
async fn integrate(
    repo: &Repo,
    git_dir: &Path,
    branch: &str,
    upstream: &Upstream,
    fetched: Option<Fetched>,
) -> Result<()> {
    let (git_dir, branch, upstream) = (git_dir.to_owned(), branch.to_owned(), upstream.clone());
    repo.exclusive(|run| async move {
        // The branch and its settings again, now that no other write can change them: one may
        // have while fetching. All that's read while holding up the other writes is read at once.
        let (current, config, before, has_head) = futures::try_join!(
            current_branch(&run),
            read_config(&run),
            progress(&run, &git_dir),
            ref_exists(&run, "HEAD"),
        )?;
        if current.as_deref() != Some(branch.as_str()) {
            return Err(Error::RepositoryChanged(format!(
                "Switched from {branch} while pulling it. Pull again."
            )));
        }
        check_still_tracks(&config, &branch, &upstream)?;
        let tracking = match upstream.tracking {
            Some(tracking) => Some(tracking),
            None => tracking_branch(&run, &branch).await?,
        };
        let Some(tracking) = tracking.filter(|tracking| !tracking.is_empty()) else {
            return Err(Error::NoUpstream(no_upstream(&branch)));
        };
        let args = integrate_args(&config, &branch, &tracking, fetched.as_ref(), has_head);

        // The merge or rebase rewrites the files the upstream changed.
        let failure = match run
            .run_with(&args, RunOptions::default().rewrites_files())
            .await
        {
            Ok(_) => None,
            Err(Error::Git(failure)) => Some(failure),
            Err(error) => return Err(error),
        };
        // Git won't merge or rebase over a merge, rebase or conflicts that were already there,
        // and says so; those aren't the pull's to explain.
        if !in_progress(&before)
            && let Some(interrupted) =
                interruption(&run, &git_dir, args.as_slice(), &tracking, failure.as_ref()).await?
        {
            return Err(interrupted);
        }
        match failure {
            Some(failure) => Err(Error::Git(failure.without_hints())),
            None => Ok(()),
        }
    })
    .await
}

/// The upstream a branch pulls from.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Upstream {
    /// The remote's name; "." for a local branch.
    remote: String,
    /// The branch's full name on the remote, e.g. `refs/heads/main`.
    merge: String,
    /// Its remote-tracking branch, e.g. `refs/remotes/origin/main`, or the local branch it is;
    /// `None` until the branch has a commit (see `tracking_branch`).
    tracking: Option<String>,
}

/// The checked-out branch, the upstream it pulls from, and the settings; fails if there isn't
/// one. `dirs` is only waited for with HEAD detached.
async fn read_branch(
    run: &GitCommand,
    dirs: impl Future<Output = Result<GitDirs>>,
) -> Result<(String, Upstream, RemoteConfig)> {
    let (branch, config) = futures::try_join!(current_branch(run), read_config(run))?;
    let Some(branch) = branch else {
        let git_dir = dirs.await?.git_dir;
        return Err(Error::NoUpstream(
            why_detached(&git_dir, Action::Pull).await,
        ));
    };
    let upstream = read_upstream(run, &branch, &config).await?;
    Ok((branch, upstream, config))
}

/// The upstream `branch` pulls from, as `config` has it; fails if there isn't one.
async fn read_upstream(run: &GitCommand, branch: &str, config: &RemoteConfig) -> Result<Upstream> {
    // From the config: a branch without commits yet has no ref to ask for its upstream. A remote
    // given as a URL, rather than one with a name (and so a URL setting), has no remote-tracking
    // branches.
    let remote = config
        .get(&format!("branch.{branch}.remote"))
        .filter(|remote| !remote.is_empty());
    let merge = config
        .get(&format!("branch.{branch}.merge"))
        .filter(|merge| !merge.is_empty());
    let (Some(remote), Some(merge)) = (remote, merge) else {
        return Err(Error::NoUpstream(no_upstream(branch)));
    };
    if remote != "." && !config.has(&format!("remote.{remote}.url")) {
        return Err(Error::NoUpstream(no_upstream(branch)));
    }
    // `git pull` merges them all at once; only one is merged here.
    if config.all(&format!("branch.{branch}.merge")).len() > 1 {
        return Err(Error::NoUpstream(format!(
            "{branch} tracks several branches, which only git pull, in a terminal, merges at once."
        )));
    }
    // The remote-tracking branch it's fetched into, known (once the branch has a commit to be
    // listed) even before the first fetch. Without one, as for an upstream given as a URL,
    // `git status` reports no upstream either, so the UI doesn't offer to pull: refused before
    // fetching.
    // Asked for by its full name, but `for-each-ref` also lists the refs under it (`main/x`, when
    // `main` has no commits): the branch's own line is picked out.
    let reference = format!("refs/heads/{branch}");
    let listed = run
        .run([
            "for-each-ref",
            "--format=%(refname)%00%(upstream)",
            &reference,
        ])
        .await?;
    let prefix = format!("{reference}\0");
    let tracking = listed
        .split('\n')
        .find_map(|entry| entry.strip_prefix(&prefix))
        .map(str::to_owned);
    if tracking.as_deref() == Some("") {
        return Err(Error::NoUpstream(no_upstream(branch)));
    }
    Ok(Upstream {
        remote: remote.to_owned(),
        merge: merge.to_owned(),
        tracking,
    })
}

/// What a pull fetched, to merge.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Fetched {
    /// The upstream's commit, as it was fetched.
    sha: String,
    /// The merge commit's message, worded by git as `git pull` would.
    message: String,
}

/// Fetches the branch's upstream as `git pull` does: its whole remote, which updates the
/// remote-tracking branches, and FETCH_HEAD, read before another fetch replaces it. Pruned too,
/// as the Fetch button does, so the remote is only asked once. A local upstream (remote ".") has
/// nothing to fetch. With what was fetched and the message to merge it with, if `word`, and
/// there's one: there's none for a branch already up to date, or without commits (which just
/// moves to its upstream). Fails if the upstream no longer exists.
async fn fetch_upstream(
    repo: &Repo,
    git_dir: &Path,
    upstream: &Upstream,
    word: bool,
) -> Result<Option<Fetched>> {
    let Upstream {
        remote,
        merge,
        tracking,
    } = upstream;
    // The upstream's commit, and its line of FETCH_HEAD: the one to merge, and the only one.
    let (sha, fetch_head) = if remote == "." {
        let Some(tracking) = tracking else {
            return Ok(None);
        };
        let Some(local) = resolve_ref(&repo.git(), tracking).await? else {
            let name = tracking.strip_prefix("refs/heads/").unwrap_or(tracking);
            return Err(Error::NoUpstream(format!("{name} no longer exists.")));
        };
        // As `git pull` words FETCH_HEAD for a local branch.
        let fetch_head = format!("{local}\t\t{} of .\n", fetched_name(merge));
        (local, fetch_head)
    } else {
        let fetch_head_file: PathBuf = git_dir.join("FETCH_HEAD");
        let remote_arg = remote.clone();
        let written = repo
            .fetching(|run| async move {
                // FETCH_HEAD is read next, so it's written whatever `fetch.writeFetchHEAD` says.
                git_fetch(
                    &run,
                    &["--quiet", "--prune", "--", &remote_arg],
                    &["fetch.writeFetchHEAD=true"],
                )
                .await?;
                let bytes = async_fs::read(&fetch_head_file).await?;
                Ok::<_, Error>(String::from_utf8_lossy(&bytes).into_owned())
            })
            .await?;
        // The upstream's line, picked by its name rather than by git marking it as the one to
        // merge: git marks the upstream of the branch checked out as it fetched, which may have
        // been another. None for an upstream deleted on the remote, whose remote-tracking branch
        // was just pruned.
        let name = format!("{} of ", fetched_name(merge));
        let line = written.split('\n').find(|entry| {
            entry
                .split('\t')
                .nth(2)
                .is_some_and(|description| description.starts_with(&name))
        });
        let Some(line) = line else {
            let short = merge.strip_prefix("refs/heads/").unwrap_or(merge);
            return Err(Error::NoUpstream(format!(
                "{short} no longer exists on {remote}."
            )));
        };
        let mut fields = line.split('\t');
        let sha = fields.next().unwrap_or("").to_owned();
        let description = fields.nth(1).unwrap_or("");
        // Marked as the one to merge.
        let fetch_head = format!("{sha}\t\t{description}\n");
        (sha, fetch_head)
    };
    if sha.is_empty() || !word {
        return Ok(None);
    }
    // Worded from FETCH_HEAD, as `git pull` has it: e.g. "Merge branch 'main' of <URL>", the URL
    // as the fetch wrote it there (without credentials). Nothing to word for a branch already up
    // to date, or without commits.
    let message = repo
        .read_with(["fmt-merge-msg"], RunOptions::default().stdin(fetch_head))
        .await
        .map(|output| output.trim().to_owned())
        .unwrap_or_default();
    Ok((!message.is_empty()).then_some(Fetched { sha, message }))
}

/// The kinds of refs `git fetch` names in FETCH_HEAD, by the prefix of their full names.
const FETCHED_KINDS: [(&str, &str); 3] = [
    ("refs/heads/", "branch"),
    ("refs/tags/", "tag"),
    ("refs/remotes/", "remote-tracking branch"),
];

/// How `git fetch` names `reference` in FETCH_HEAD, before " of <URL>": e.g. "branch 'main'".
/// Also how `git merge` names what it merges, for the message `git fmt-merge-msg` words from it.
pub fn fetched_name(reference: &str) -> String {
    for (prefix, kind) in FETCHED_KINDS {
        if let Some(name) = reference.strip_prefix(prefix) {
            return format!("{kind} '{name}'");
        }
    }
    format!("'{reference}'")
}

/// The remote-tracking branch `branch`'s upstream was fetched into, for a branch without commits,
/// which `for-each-ref` doesn't list: it's only found once that exists, after the fetch. `None`
/// without one.
async fn tracking_branch(run: &GitCommand, branch: &str) -> Result<Option<String>> {
    let upstream = format!("{branch}@{{upstream}}");
    match run
        .run(["rev-parse", "--symbolic-full-name", &upstream])
        .await
    {
        Ok(reference) => {
            let reference = reference.trim();
            Ok((!reference.is_empty()).then(|| reference.to_owned()))
        }
        // Git says there's no upstream, or none fetched; anything else (no longer a repository,
        // say, a GitError of its own kind) is for the caller.
        Err(Error::Git(GitError {
            kind: GitErrorKind::Command,
            ref stderr,
            ..
        })) if ["upstream", "remote-tracking", "unknown revision"]
            .iter()
            .any(|said| stderr.contains(said)) =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

/// The merge or rebase `git pull` would do after fetching, going by the same settings. Unlike
/// `git pull`, which refuses to pull a branch that diverged from its upstream until told whether
/// to merge or rebase, this merges then, like git did by default before 2.27. And it never
/// rebases interactively, which would open an editor with no one to use it.
fn integrate_args(
    config: &RemoteConfig,
    branch: &str,
    tracking: &str,
    fetched: Option<&Fetched>,
    has_head: bool,
) -> Vec<String> {
    let ff = config.get("pull.ff").map(str::to_lowercase);
    // A branch without commits yet has nothing to rebase: it's merged, which just moves it there.
    if has_head && rebases(config, branch) {
        let rebase = rebase_setting(config, branch);
        let mut args = vec!["rebase".to_owned(), "--quiet".to_owned()];
        if matches!(rebase.as_deref(), Some("merges" | "m")) {
            args.push("--rebase-merges".to_owned());
        }
        // From where the branch forked from its upstream, like `git pull --rebase`.
        args.push("--fork-point".to_owned());
        args.push(tracking.to_owned());
        return args;
    }
    let ff_args: &[&str] = match ff.as_deref() {
        _ if !has_head => &[],
        None => &[],
        Some("only") => &["--ff-only"],
        Some(ff) if is_false(ff) => &["--no-ff"],
        Some(_) => &["--ff"],
    };
    let mut args: Vec<String> = ["merge", "--quiet", "--no-edit"].map(str::to_owned).into();
    // Its log, if `merge.log` wants one, is already in the message, and its comments are git's
    // own.
    if fetched.is_some() {
        args.push("--no-log".to_owned());
        args.push("--cleanup=strip".to_owned());
    }
    args.extend(ff_args.iter().map(|arg| (*arg).to_owned()));
    // What was fetched, with the message `git pull` would give it; or, without one, the
    // remote-tracking branch.
    match fetched {
        Some(fetched) => {
            args.push("-m".to_owned());
            args.push(fetched.message.clone());
            args.push(fetched.sha.clone());
        }
        None => args.push(tracking.to_owned()),
    }
    args
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use regex::Regex;

    use super::*;
    use crate::core::repo::{How, Observed};
    use crate::features::remote::blockers::{no_branch, rebasing};
    use crate::test_support::conflicts::try_git;
    use crate::test_support::{Fixture, git, valid};

    /// Writes `content` to `file` in `path` and commits it with `message`.
    fn commit(path: &Path, file: &str, content: &str, message: &str) {
        fs::write(path.join(file), content).unwrap();
        git(path, ["add", file]);
        git(path, ["commit", "-qm", message]);
    }

    fn subjects(path: &Path) -> Vec<String> {
        lines(&git(path, ["log", "--format=%s"]))
    }

    fn lines(text: &str) -> Vec<String> {
        text.split('\n').map(str::to_owned).collect()
    }

    /// Asserts that `error` is `expected`: the same kind, with the same message.
    #[track_caller]
    fn assert_error(error: &Error, expected: Error) {
        assert_eq!(
            std::mem::discriminant(error),
            std::mem::discriminant(&expected),
            "{error:?} isn't {expected:?}"
        );
        assert_eq!(error.to_string(), expected.to_string());
    }

    /// Waits for the fetches queued for `repo`, as a pull queues the other remotes' once merged.
    async fn fetches_done(repo: &Repo) {
        repo.fetching(|_| async {}).await;
    }

    #[test]
    fn the_commands_can_run_on_any_thread() {
        fn send<T: Send>(_: T) {}
        let fx = Fixture::new();
        fx.create_repo("send");
        let repo = smol::block_on(fx.repos.open("send")).unwrap();
        send(pull(&repo));
        send(pull_branch(&repo, &valid("main")));
        send(fetch_all(&repo));
        send(list_remotes(&repo.git()));
        send(crate::features::remote::push::push(&repo, false));
        send(crate::features::remote::push::get_push_target(&repo));
    }

    #[test]
    fn lists_the_remotes_by_name() {
        let fx = Fixture::new();
        let (upstream, path) = fx.create_clone("listed");
        git(
            &path,
            ["remote", "add", "other", upstream.to_str().unwrap()],
        );
        smol::block_on(async {
            let repo = fx.repos.open("listed").await.unwrap();
            assert_eq!(
                list_remotes(&repo.git()).await.unwrap(),
                ["origin", "other"]
            );
        });
    }

    #[test]
    fn fetched_name_names_a_ref_as_git_fetch_does() {
        assert_eq!(fetched_name("refs/heads/main"), "branch 'main'");
        assert_eq!(fetched_name("refs/tags/v1"), "tag 'v1'");
        assert_eq!(
            fetched_name("refs/remotes/origin/x"),
            "remote-tracking branch 'origin/x'"
        );
        assert_eq!(fetched_name("HEAD"), "'HEAD'");
    }

    /// `upstream` plays the remote: a repository the clone fetches from as `origin`. Returns the
    /// upstream's path and the clone's, which `repos` opens as `clone`.
    fn fetch_all_repos(fx: &Fixture) -> (PathBuf, PathBuf) {
        let upstream = fx.create_repo("upstream");
        git(&upstream, ["commit", "--allow-empty", "-qm", "first"]);
        git(&upstream, ["branch", "feature"]);
        let path = fx.clone_repo("clone", &upstream);
        (upstream, path)
    }

    #[test]
    fn fetch_all_fetches_new_commits_and_drops_branches_deleted_on_the_remote() {
        let fx = Fixture::new();
        let (upstream, path) = fetch_all_repos(&fx);
        git(&upstream, ["commit", "--allow-empty", "-qm", "second"]);
        git(&upstream, ["branch", "-D", "feature"]);
        smol::block_on(async {
            fetch_all(&fx.repos.open("clone").await.unwrap())
                .await
                .unwrap();
        });
        assert_eq!(
            lines(&git(
                &path,
                [
                    "for-each-ref",
                    "--format=%(refname:lstrip=2) %(subject)",
                    "refs/remotes"
                ]
            )),
            ["origin/HEAD second", "origin/main second"]
        );
    }

    #[test]
    fn fetch_all_does_nothing_without_remotes() {
        let fx = Fixture::new();
        fx.create_repo("lonely");
        smol::block_on(async {
            fetch_all(&fx.repos.open("lonely").await.unwrap())
                .await
                .unwrap();
        });
    }

    #[test]
    fn fetch_all_explains_why_a_fetch_failed() {
        let fx = Fixture::new();
        let (_, path) = fetch_all_repos(&fx);
        let gone = fx.root().join("gone");
        git(
            &path,
            ["remote", "set-url", "origin", gone.to_str().unwrap()],
        );
        smol::block_on(async {
            let error = fetch_all(&fx.repos.open("clone").await.unwrap())
                .await
                .unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("does not appear to be a git repository"),
                "{error}"
            );
        });
    }

    /// Fetches from a remote held up by `gate_remote`: let through by `release`, or once dropped.
    struct Gate {
        gate: PathBuf,
        log: PathBuf,
    }

    impl Gate {
        fn release(&self) {
            fs::write(&self.gate, "").unwrap();
        }

        /// How many fetches from the remote started.
        fn count(&self) -> usize {
            fs::read_to_string(&self.log).unwrap().len()
        }
    }

    impl Drop for Gate {
        fn drop(&mut self) {
            let _ = fs::write(&self.gate, "");
        }
    }

    /// Has fetches from the remote `name` of the repository at `path` wait until `release` is
    /// called (or the gate is dropped), and counts them: each one starts upload-pack on the
    /// remote.
    fn gate_remote(path: &Path, name: &str) -> Gate {
        let gate = PathBuf::from(format!("{}-{name}-gate", path.display()));
        let log = PathBuf::from(format!("{}-{name}-fetches", path.display()));
        fs::write(&log, "").unwrap();
        git(
            path,
            [
                "config".to_owned(),
                format!("remote.{name}.uploadpack"),
                format!(
                    "f() {{ echo >> '{}'; until [ -e '{}' ]; do sleep 0.02; done; git-upload-pack \"$@\"; }}; f",
                    log.display(),
                    gate.display()
                ),
            ],
        );
        Gate { gate, log }
    }

    #[test]
    fn pull_fast_forwards_to_the_upstreams_new_commits() {
        let fx = Fixture::new();
        let (upstream, path) = fx.create_clone("behind");
        commit(&upstream, "a.txt", "a\nb\n", "second");
        smol::block_on(async {
            let repo = fx.repos.open("behind").await.unwrap();
            pull(&repo).await.unwrap();
            assert_eq!(subjects(&path), ["second", "first"]);
            assert_eq!(fs::read_to_string(path.join("a.txt")).unwrap(), "a\nb\n");
            fetches_done(&repo).await;
        });
    }

    #[test]
    fn pull_fetches_every_remote_dropping_branches_deleted_on_it_as_the_fetch_button_does() {
        let fx = Fixture::new();
        let (upstream, path) = fx.create_clone("pruned");
        git(&upstream, ["branch", "old"]);
        git(&path, ["fetch", "-q"]);
        git(&upstream, ["branch", "-D", "old"]);
        git(&upstream, ["branch", "new"]);
        let other = fx.create_repo("pruned-other");
        commit(&other, "b.txt", "b\n", "other");
        git(&path, ["remote", "add", "other", other.to_str().unwrap()]);
        // As `git fetch --all` does, without the remotes set to be skipped.
        git(&path, ["remote", "add", "skipped", other.to_str().unwrap()]);
        git(&path, ["config", "remote.skipped.skipFetchAll", "true"]);
        smol::block_on(async {
            let repo = fx.repos.open("pruned").await.unwrap();
            pull(&repo).await.unwrap();
            fetches_done(&repo).await;
        });
        let branches = git(
            &path,
            [
                "for-each-ref",
                "--format=%(refname:lstrip=2)",
                "refs/remotes",
            ],
        );
        // Without the remotes' HEADs, which newer git also records on a first fetch.
        let branches: Vec<&str> = branches
            .split('\n')
            .filter(|branch| !branch.ends_with("/HEAD"))
            .collect();
        assert_eq!(branches, ["origin/main", "origin/new", "other/main"]);
    }

    #[test]
    fn pull_asks_the_upstreams_remote_once_and_doesnt_wait_for_the_others() {
        let fx = Fixture::new();
        let (upstream, path) = fx.create_clone("once");
        let origin = gate_remote(&path, "origin");
        origin.release();
        let other = fx.create_repo("once-other");
        commit(&other, "b.txt", "b\n", "other");
        git(&path, ["remote", "add", "other", other.to_str().unwrap()]);
        let slow = gate_remote(&path, "other");
        commit(&upstream, "a.txt", "a\nb\n", "second");
        smol::block_on(async {
            let repo = fx.repos.open("once").await.unwrap();
            pull(&repo).await.unwrap();
            assert_eq!(subjects(&path), ["second", "first"]);
            assert_eq!(origin.count(), 1);
            slow.release();
            fetches_done(&repo).await;
            assert!(
                git(
                    &path,
                    [
                        "for-each-ref",
                        "--format=%(refname:lstrip=2)",
                        "refs/remotes"
                    ]
                )
                .contains("other/main")
            );
            assert_eq!(origin.count(), 1);
        });
    }

    #[test]
    fn pull_says_when_the_upstream_was_deleted_on_the_remote_dropping_its_remote_tracking_branch() {
        let fx = Fixture::new();
        let (upstream, path) = fx.create_clone("deleted-upstream");
        git(&upstream, ["branch", "gone"]);
        git(&path, ["fetch", "-q"]);
        git(&path, ["checkout", "-qb", "gone", "--track", "origin/gone"]);
        git(&upstream, ["branch", "-D", "gone"]);
        assert_error(
            &failed_pull(&fx, "deleted-upstream"),
            Error::NoUpstream("gone no longer exists on origin.".into()),
        );
        assert_eq!(git(&path, ["for-each-ref", "refs/remotes/origin/gone"]), "");
    }

    #[test]
    fn pull_pulls_even_when_another_remote_cant_be_reached() {
        let fx = Fixture::new();
        let (upstream, path) = fx.create_clone("unreachable");
        let gone = fx.root().join("gone");
        git(&path, ["remote", "add", "gone", gone.to_str().unwrap()]);
        commit(&upstream, "a.txt", "a\nb\n", "second");
        smol::block_on(async {
            let repo = fx.repos.open("unreachable").await.unwrap();
            pull(&repo).await.unwrap();
            assert_eq!(subjects(&path), ["second", "first"]);
            fetches_done(&repo).await;
        });
    }

    #[test]
    fn pull_fetches_every_remote_when_pulling_from_a_local_upstream() {
        let fx = Fixture::new();
        let (upstream, path) = fx.create_clone("local-pruned");
        git(&path, ["checkout", "-qb", "feature", "--track", "main"]);
        git(&upstream, ["branch", "new"]);
        smol::block_on(async {
            let repo = fx.repos.open("local-pruned").await.unwrap();
            pull(&repo).await.unwrap();
            fetches_done(&repo).await;
        });
        assert!(
            git(
                &path,
                [
                    "for-each-ref",
                    "--format=%(refname:lstrip=2)",
                    "refs/remotes"
                ]
            )
            .contains("origin/new")
        );
    }

    /// Pulls into the repository `name`, which succeeds.
    fn pulled(fx: &Fixture, name: &str) {
        smol::block_on(async {
            let repo = fx.repos.open(name).await.unwrap();
            pull(&repo).await.unwrap();
            fetches_done(&repo).await;
        });
    }

    /// Pulls into the repository `name`, which fails; with what it failed with.
    fn failed_pull(fx: &Fixture, name: &str) -> Error {
        smol::block_on(async {
            let repo = fx.repos.open(name).await.unwrap();
            let error = pull(&repo).await.unwrap_err();
            fetches_done(&repo).await;
            error
        })
    }

    #[test]
    fn pull_does_nothing_when_theres_nothing_new() {
        let fx = Fixture::new();
        let (_, path) = fx.create_clone("current");
        pulled(&fx, "current");
        assert_eq!(subjects(&path), ["first"]);
    }

    #[test]
    fn pull_merges_a_branch_that_diverged_from_its_upstream() {
        let fx = Fixture::new();
        let (upstream, path) = fx.create_clone("diverged");
        commit(&upstream, "theirs.txt", "theirs\n", "theirs");
        commit(&path, "ours.txt", "ours\n", "ours");
        pulled(&fx, "diverged");
        assert_eq!(
            git(&path, ["log", "-1", "--format=%P"]).split(' ').count(),
            2
        );
        let subjects = subjects(&path);
        for subject in ["theirs", "ours", "first"] {
            assert!(subjects.iter().any(|s| s == subject), "{subjects:?}");
        }
    }

    #[test]
    fn pull_words_the_merge_commit_as_git_pull_does() {
        let fx = Fixture::new();
        let (upstream, path) = fx.create_clone("worded");
        git(
            &path,
            ["checkout", "-qb", "feature", "--track", "origin/main"],
        );
        let twin = fx.clone_repo("worded-twin", &upstream);
        git(
            &twin,
            ["checkout", "-qb", "feature", "--track", "origin/main"],
        );
        commit(&upstream, "theirs.txt", "theirs\n", "theirs");
        commit(&path, "ours.txt", "ours\n", "ours");
        commit(&twin, "ours.txt", "ours\n", "ours");

        // With the log of what's merged, which git adds to its message.
        git(&path, ["config", "merge.log", "true"]);
        git(&twin, ["config", "merge.log", "true"]);
        pulled(&fx, "worded");
        git(&twin, ["pull", "-q", "--no-rebase"]);
        assert_eq!(
            git(&path, ["log", "-1", "--format=%s"]),
            format!("Merge branch 'main' of {} into feature", upstream.display())
        );
        assert_eq!(
            git(&path, ["log", "-1", "--format=%B"]),
            git(&twin, ["log", "-1", "--format=%B"])
        );
    }

    #[test]
    fn pull_says_when_a_merge_is_left_to_commit() {
        let fx = Fixture::new();
        let (upstream, path) = fx.create_clone("no-commit");
        git(&path, ["config", "branch.main.mergeOptions", "--no-commit"]);
        commit(&upstream, "theirs.txt", "theirs\n", "theirs");
        commit(&path, "ours.txt", "ours\n", "ours");
        assert_error(
            &failed_pull(&fx, "no-commit"),
            Error::PullInterrupted(
                "Merging origin/main stopped before committing. Commit the merge, or abort it."
                    .into(),
            ),
        );
    }

    #[test]
    fn pull_says_when_a_merge_is_squashed_into_the_staged_changes() {
        let fx = Fixture::new();
        let (upstream, path) = fx.create_clone("squash");
        git(&path, ["config", "branch.main.mergeOptions", "--squash"]);
        commit(&upstream, "theirs.txt", "theirs\n", "theirs");
        commit(&path, "ours.txt", "ours\n", "ours");
        assert_error(
            &failed_pull(&fx, "squash"),
            Error::PullInterrupted(
                "Squashed origin/main into the staged changes, without committing. Commit them to finish pulling.".into(),
            ),
        );
    }

    #[test]
    fn pull_says_to_commit_a_squash_that_stopped_at_conflicts() {
        let fx = Fixture::new();
        let (upstream, path) = fx.create_clone("squash-conflict");
        git(&path, ["config", "branch.main.mergeOptions", "--squash"]);
        commit(&upstream, "a.txt", "theirs\n", "theirs");
        commit(&path, "a.txt", "ours\n", "ours");
        assert_error(
            &failed_pull(&fx, "squash-conflict"),
            Error::PullInterrupted(
                "Pulling origin/main caused conflicts. Resolve them, then commit the squashed changes.".into(),
            ),
        );
    }

    #[test]
    fn pull_isnt_put_off_by_a_squash_message_left_behind() {
        let fx = Fixture::new();
        let (upstream, path) = fx.create_clone("stale-squash");
        fs::write(path.join(".git").join("SQUASH_MSG"), "an old squash\n").unwrap();
        commit(&upstream, "a.txt", "theirs\n", "theirs");
        commit(&path, "a.txt", "ours\n", "ours");
        assert_error(
            &failed_pull(&fx, "stale-squash"),
            Error::PullInterrupted(
                "Pulling origin/main caused conflicts. Resolve them, then commit the merge.".into(),
            ),
        );
    }

    #[test]
    fn pull_says_when_a_local_upstream_no_longer_exists() {
        let fx = Fixture::new();
        let path = fx.create_repo("gone-upstream");
        commit(&path, "a.txt", "a\n", "first");
        git(&path, ["branch", "other"]);
        git(&path, ["branch", "-q", "--set-upstream-to", "other"]);
        git(&path, ["branch", "-D", "other"]);
        assert_error(
            &failed_pull(&fx, "gone-upstream"),
            Error::NoUpstream("other no longer exists.".into()),
        );
    }

    #[test]
    fn pull_leaves_a_branch_that_tracks_several_to_git_pull() {
        let fx = Fixture::new();
        let (_, path) = fx.create_clone("octopus");
        git(
            &path,
            ["config", "--add", "branch.main.merge", "refs/heads/other"],
        );
        assert_error(
            &failed_pull(&fx, "octopus"),
            Error::NoUpstream(
                "main tracks several branches, which only git pull, in a terminal, merges at once."
                    .into(),
            ),
        );
    }

    #[test]
    fn pull_merges_what_it_fetched_even_when_told_not_to_write_fetch_head() {
        let fx = Fixture::new();
        let (upstream, path) = fx.create_clone("no-fetch-head");
        git(&path, ["config", "fetch.writeFetchHEAD", "false"]);
        commit(&upstream, "theirs.txt", "theirs\n", "theirs");
        commit(&path, "ours.txt", "ours\n", "ours");
        pulled(&fx, "no-fetch-head");
        assert_eq!(
            git(&path, ["log", "-1", "--format=%s"]),
            format!("Merge branch 'main' of {}", upstream.display())
        );
        assert!(subjects(&path).contains(&"theirs".to_owned()));
    }

    #[test]
    fn pull_rebases_when_the_config_says_to() {
        let fx = Fixture::new();
        let (upstream, path) = fx.create_clone("rebased");
        git(&path, ["config", "pull.rebase", "true"]);
        commit(&upstream, "theirs.txt", "theirs\n", "theirs");
        commit(&path, "ours.txt", "ours\n", "ours");
        pulled(&fx, "rebased");
        assert_eq!(subjects(&path), ["ours", "theirs", "first"]);
    }

    #[test]
    fn pull_leaves_conflicts_to_resolve() {
        let fx = Fixture::new();
        let (upstream, path) = fx.create_clone("conflicted");
        commit(&upstream, "a.txt", "theirs\n", "theirs");
        commit(&path, "a.txt", "ours\n", "ours");
        assert_error(
            &failed_pull(&fx, "conflicted"),
            Error::PullInterrupted(
                "Pulling origin/main caused conflicts. Resolve them, then commit the merge.".into(),
            ),
        );
        assert_ne!(git(&path, ["ls-files", "--unmerged"]), "");

        // Pulling again isn't the conflict's cause; git says why it can't.
        let again = failed_pull(&fx, "conflicted");
        assert!(!matches!(again, Error::PullInterrupted(_)), "{again:?}");
        assert!(again.to_string().contains("unmerged files"), "{again}");
    }

    #[test]
    fn pull_says_to_continue_a_rebase_that_stopped_at_conflicts() {
        let fx = Fixture::new();
        let (upstream, path) = fx.create_clone("rebase-conflict");
        git(&path, ["config", "pull.rebase", "true"]);
        commit(&upstream, "a.txt", "theirs\n", "theirs");
        commit(&path, "a.txt", "ours\n", "ours");
        assert_eq!(
            failed_pull(&fx, "rebase-conflict").to_string(),
            "Pulling origin/main caused conflicts. Resolve them, then continue the rebase."
        );
        // HEAD is detached until the rebase is done; that's what's in the way of pulling again.
        assert_error(
            &failed_pull(&fx, "rebase-conflict"),
            Error::NoUpstream(rebasing()),
        );
    }

    #[test]
    fn pull_rebases_without_opening_an_editor_when_the_config_says_to_rebase_interactively() {
        let fx = Fixture::new();
        let (upstream, path) = fx.create_clone("interactive");
        git(&path, ["config", "pull.rebase", "interactive"]);
        commit(&upstream, "theirs.txt", "theirs\n", "theirs");
        commit(&path, "ours.txt", "ours\n", "ours");
        // An editor that fails, so an interactive rebase would too. Set in the repository's
        // config rather than the environment, which the tests share: the sequence editor's
        // setting goes before GIT_EDITOR, which may be set.
        git(&path, ["config", "core.editor", "false"]);
        git(&path, ["config", "sequence.editor", "false"]);
        pulled(&fx, "interactive");
        assert_eq!(subjects(&path), ["ours", "theirs", "first"]);
    }

    #[test]
    fn pull_says_when_stashed_local_changes_conflict_with_what_it_pulled() {
        let fx = Fixture::new();
        let (upstream, path) = fx.create_clone("autostash");
        git(&path, ["config", "pull.rebase", "true"]);
        git(&path, ["config", "rebase.autoStash", "true"]);
        commit(&upstream, "a.txt", "theirs\n", "theirs");
        fs::write(path.join("a.txt"), "uncommitted\n").unwrap();
        assert_error(
            &failed_pull(&fx, "autostash"),
            Error::PullInterrupted(
                "Pulled origin/main, but your local changes conflict with it. Resolve the conflicts; your changes are also kept in the stash.".into(),
            ),
        );
        assert_eq!(subjects(&path), ["theirs", "first"]);
    }

    #[test]
    fn pull_doesnt_rebase_when_told_to_only_fast_forward() {
        let fx = Fixture::new();
        let (upstream, path) = fx.create_clone("ff-only-rebase");
        git(&path, ["config", "pull.rebase", "true"]);
        git(&path, ["config", "pull.ff", "only"]);
        commit(&upstream, "theirs.txt", "theirs\n", "theirs");
        commit(&path, "ours.txt", "ours\n", "ours");
        let ours = git(&path, ["rev-parse", "HEAD"]);
        assert_eq!(
            failed_pull(&fx, "ff-only-rebase").to_string(),
            "fatal: Not possible to fast-forward, aborting."
        );
        assert_eq!(git(&path, ["rev-parse", "HEAD"]), ours);
    }

    #[test]
    fn pull_leaves_gits_hints_out_of_why_it_failed() {
        let fx = Fixture::new();
        let (upstream, path) = fx.create_clone("ff-only");
        git(&path, ["config", "pull.ff", "only"]);
        commit(&upstream, "theirs.txt", "theirs\n", "theirs");
        commit(&path, "ours.txt", "ours\n", "ours");
        assert_eq!(
            failed_pull(&fx, "ff-only").to_string(),
            "fatal: Not possible to fast-forward, aborting."
        );
    }

    /// The process group of this process, as `ps` says.
    #[cfg(unix)]
    fn our_process_group() -> String {
        let output = std::process::Command::new("ps")
            .args(["-o", "pgid=", "-p", &std::process::id().to_string()])
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    #[cfg(unix)]
    fn write_script(file: &Path, script: &str) {
        use std::os::unix::fs::PermissionsExt;
        fs::write(file, script).unwrap();
        fs::set_permissions(file, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn pull_fetches_without_a_terminal_for_ssh_to_ask_questions_on_with_the_users_ssh_command() {
        let fx = Fixture::new();
        // An `ssh` that records its process group (a session of its own has its own) and how it
        // was run, and fails. Named in the user's ssh command by its path, rather than put on the
        // PATH, which the tests share.
        let bin = fx.root().join("bin");
        let log = fx.root().join("ssh.log");
        fs::create_dir_all(&bin).unwrap();
        write_script(
            &bin.join("ssh"),
            &format!(
                "#!/bin/sh\necho \"$(ps -o pgid= -p $$) $*\" >> \"{}\"\nexit 1\n",
                log.display()
            ),
        );

        let (_, path) = fx.create_clone("ssh");
        git(
            &path,
            [
                "remote",
                "set-url",
                "origin",
                "git@example.invalid:repo.git",
            ],
        );
        git(
            &path,
            [
                "config".to_owned(),
                "core.sshCommand".to_owned(),
                format!("'{}' -o User=me", bin.join("ssh").display()),
            ],
        );
        let error = failed_pull(&fx, "ssh");
        assert!(matches!(error, Error::Git(_)), "{error:?}");

        let logged = fs::read_to_string(&log).unwrap();
        let mut fields = logged.split_whitespace();
        let group = fields.next().unwrap();
        let args = fields.collect::<Vec<_>>().join(" ");
        assert_ne!(group, our_process_group());
        assert!(args.contains("-o User=me"), "{args}");
    }

    #[cfg(unix)]
    #[test]
    fn pull_says_why_a_merge_stopped_before_committing_and_merges_without_a_terminal() {
        let fx = Fixture::new();
        let (upstream, path) = fx.create_clone("hooked");
        commit(&upstream, "theirs.txt", "theirs\n", "theirs");
        commit(&path, "ours.txt", "ours\n", "ours");
        // A hook that records its process group, and turns the merge commit down.
        let log = fx.root().join("hook.log");
        let hooks = path.join(".git").join("hooks");
        fs::create_dir_all(&hooks).unwrap();
        write_script(
            &hooks.join("pre-merge-commit"),
            &format!(
                "#!/bin/sh\nps -o pgid= -p $$ > \"{}\"\necho \"Not today.\" >&2\nexit 1\n",
                log.display()
            ),
        );

        let error = failed_pull(&fx, "hooked");
        assert!(matches!(error, Error::PullInterrupted(_)), "{error:?}");
        let pattern = Regex::new(
            r"(?s)^Merging origin/main stopped before committing:\nNot today\.\n.*Fix that, then commit the merge, or abort it\.$",
        )
        .unwrap();
        assert!(pattern.is_match(&error.to_string()), "{error}");
        assert_ne!(
            fs::read_to_string(&log).unwrap().trim(),
            our_process_group()
        );
    }

    /// `repo`, counting the fetches it runs through `Repo::fetching` (TS's test wrapped
    /// `fetching` itself, which runs one fetch each time).
    fn counting_fetches(repo: &Repo) -> (Repo, Arc<AtomicUsize>) {
        let fetches = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&fetches);
        let repo = repo.observed(move |what| {
            if matches!(
                what,
                Observed::Ran {
                    how: How::Fetching,
                    ..
                }
            ) {
                counted.fetch_add(1, Ordering::SeqCst);
            }
        });
        (repo, fetches)
    }

    /// `repo`, running `then` once each fetch of `Repo::fetching` is done, as TS's tests did
    /// after `fetching`.
    fn after_fetching(repo: &Repo, then: impl Fn() + Send + Sync + 'static) -> Repo {
        repo.observed(move |what| {
            if let Observed::Ran {
                how: How::Fetching,
                args,
            } = what
                && args.first().map(String::as_str) == Some("fetch")
            {
                then();
            }
        })
    }

    #[test]
    fn pull_doesnt_fetch_an_upstream_it_couldnt_merge() {
        let fx = Fixture::new();
        let (upstream, path) = fx.create_clone("not-fetched");
        let upstream = upstream.to_str().unwrap().to_owned();
        git(&path, ["config", "branch.main.remote", &upstream]);
        smol::block_on(async {
            let real = fx.repos.open("not-fetched").await.unwrap();
            let (repo, fetches) = counting_fetches(&real);
            let error = pull(&repo).await.unwrap_err();
            assert!(matches!(error, Error::NoUpstream(_)), "{error:?}");

            // Not even from a branch without commits, which has no ref to ask for its upstream.
            git(&path, ["checkout", "-q", "--orphan", "new"]);
            git(&path, ["config", "branch.new.remote", &upstream]);
            git(&path, ["config", "branch.new.merge", "refs/heads/main"]);
            assert_error(
                &pull(&repo).await.unwrap_err(),
                Error::NoUpstream("new doesn't track a remote branch.".into()),
            );
            assert_eq!(fetches.load(Ordering::SeqCst), 0);
            fetches_done(&repo).await;
        });
    }

    #[test]
    fn pull_doesnt_take_patches_being_applied_for_a_rebase() {
        let fx = Fixture::new();
        let (_, path) = fx.create_clone("am");
        git(&path, ["checkout", "-q", "--detach"]);
        // A patch that doesn't apply leaves `git am` stopped.
        let patch = fx.root().join("bad.patch");
        fs::write(
            &patch,
            "From: A <a@example.com>\nSubject: [PATCH] bad\n\n---\n--- a/missing.txt\n+++ b/missing.txt\n@@ -1 +1 @@\n-x\n+y\n",
        )
        .unwrap();
        assert_ne!(try_git(&path, ["am", patch.to_str().unwrap()]), 0);
        assert_error(&failed_pull(&fx, "am"), Error::NoUpstream(no_branch()));
    }

    #[test]
    fn pull_says_how_to_finish_a_rebase_that_stopped_without_conflicts() {
        let fx = Fixture::new();
        let (upstream, path) = fx.create_clone("rebase-stopped");
        git(&path, ["config", "pull.rebase", "true"]);
        commit(&upstream, "theirs.txt", "theirs\n", "theirs");
        // Replaying the first commit would overwrite the untracked file the second one leaves.
        commit(&path, "gen.txt", "gen\n", "add gen");
        git(&path, ["rm", "-q", "--cached", "gen.txt"]);
        git(&path, ["commit", "-qm", "untrack gen"]);
        fs::write(path.join("gen.txt"), "untracked\n").unwrap();
        let error = failed_pull(&fx, "rebase-stopped");
        assert!(matches!(error, Error::PullInterrupted(_)), "{error:?}");
        let message = error.to_string();
        let stopped = Regex::new(
            r"^Rebasing onto origin/main stopped partway:\n.*untracked working tree files would be overwritten",
        )
        .unwrap();
        assert!(stopped.is_match(&message), "{message}");
        assert!(
            message.ends_with("Fix that, then continue the rebase, or abort it."),
            "{message}"
        );
        assert!(!message.contains("hint:"), "{message}");
    }

    #[test]
    fn pull_pulls_a_branch_that_has_a_tag_of_the_same_name() {
        let fx = Fixture::new();
        let (upstream, path) = fx.create_clone("tagged");
        git(&path, ["tag", "main"]);
        commit(&upstream, "a.txt", "a\nb\n", "second");
        pulled(&fx, "tagged");
        assert_eq!(subjects(&path), ["second", "first"]);
    }

    #[test]
    fn pull_pulls_from_a_local_upstream() {
        let fx = Fixture::new();
        let path = fx.create_repo("local-upstream");
        commit(&path, "a.txt", "a\n", "first");
        git(&path, ["checkout", "-qb", "feature", "--track", "main"]);
        git(&path, ["checkout", "-q", "main"]);
        commit(&path, "a.txt", "a\nb\n", "second");
        git(&path, ["checkout", "-q", "feature"]);
        pulled(&fx, "local-upstream");
        assert_eq!(subjects(&path), ["second", "first"]);
    }

    #[test]
    fn pull_doesnt_merge_into_another_branch_checked_out_while_it_fetched() {
        let fx = Fixture::new();
        let (upstream, path) = fx.create_clone("switched");
        commit(&upstream, "a.txt", "a\nb\n", "second");
        git(&path, ["branch", "-q", "--track", "other", "origin/main"]);
        smol::block_on(async {
            let real = fx.repos.open("switched").await.unwrap();
            let switching = path.clone();
            let repo = after_fetching(&real, move || {
                git(&switching, ["checkout", "-q", "other"]);
            });
            assert_error(
                &pull(&repo).await.unwrap_err(),
                Error::RepositoryChanged("Switched from main while pulling it. Pull again.".into()),
            );
            fetches_done(&repo).await;
        });
        assert_eq!(subjects(&path), ["first"]);
    }

    #[test]
    fn pull_says_to_continue_a_rebase_that_stopped_in_a_linked_worktree() {
        let fx = Fixture::new();
        let (upstream, path) = fx.create_clone("worktree-main");
        commit(&upstream, "a.txt", "theirs\n", "theirs");
        let worktree = fx.root().join("worktree");
        git(
            &path,
            [
                "worktree",
                "add",
                "-q",
                "-b",
                "wt",
                worktree.to_str().unwrap(),
                "origin/main~0",
            ],
        );
        git(
            &worktree,
            ["branch", "-q", "--set-upstream-to", "origin/main"],
        );
        git(&worktree, ["reset", "-q", "--hard", "main"]);
        git(&worktree, ["config", "pull.rebase", "true"]);
        commit(&worktree, "a.txt", "ours\n", "ours");
        fx.add_path("worktree", &worktree);
        assert_eq!(
            failed_pull(&fx, "worktree").to_string(),
            "Pulling origin/main caused conflicts. Resolve them, then continue the rebase."
        );
    }

    #[test]
    fn pull_needs_an_upstream_with_a_remote_tracking_branch_as_git_status_does() {
        let fx = Fixture::new();
        let (upstream, path) = fx.create_clone("by-url");
        git(
            &path,
            ["config", "branch.main.remote", upstream.to_str().unwrap()],
        );
        assert_error(
            &failed_pull(&fx, "by-url"),
            Error::NoUpstream("main doesn't track a remote branch.".into()),
        );
        assert!(!git(&path, ["status", "--porcelain=v2", "--branch"]).contains("branch.upstream"));
    }

    #[test]
    fn pull_pulls_the_first_commits_into_a_branch_without_any_even_when_set_to_rebase() {
        let fx = Fixture::new();
        let upstream = fx.create_repo("empty-upstream");
        let path = fx.clone_repo("empty", &upstream);
        git(&path, ["config", "pull.rebase", "true"]);
        git(&path, ["config", "branch.main.remote", "origin"]);
        git(&path, ["config", "branch.main.merge", "refs/heads/main"]);
        commit(&upstream, "a.txt", "a\n", "first");
        pulled(&fx, "empty");
        assert_eq!(subjects(&path), ["first"]);
    }

    #[test]
    fn pull_doesnt_merge_another_upstream_than_the_one_it_fetched() {
        let fx = Fixture::new();
        let (upstream, path) = fx.create_clone("upstream-changed");
        commit(&upstream, "a.txt", "a\nb\n", "second");
        smol::block_on(async {
            let real = fx.repos.open("upstream-changed").await.unwrap();
            let changing = path.clone();
            let repo = after_fetching(&real, move || {
                git(
                    &changing,
                    ["config", "branch.main.merge", "refs/heads/release"],
                );
            });
            assert_error(
                &pull(&repo).await.unwrap_err(),
                Error::RepositoryChanged(
                    "main's upstream changed while pulling it. Pull again.".into(),
                ),
            );
            fetches_done(&repo).await;
        });
        assert_eq!(subjects(&path), ["first"]);
    }

    #[test]
    fn pull_explains_why_local_changes_stop_it_without_gits_hints() {
        let fx = Fixture::new();
        let (upstream, path) = fx.create_clone("dirty");
        commit(&upstream, "a.txt", "theirs\n", "theirs");
        fs::write(path.join("a.txt"), "uncommitted\n").unwrap();
        let error = failed_pull(&fx, "dirty");
        assert!(
            error
                .to_string()
                .contains("Your local changes to the following files would be"),
            "{error}"
        );
        assert!(!matches!(error, Error::PullInterrupted(_)), "{error:?}");
        assert_eq!(
            fs::read_to_string(path.join("a.txt")).unwrap(),
            "uncommitted\n"
        );
    }

    #[test]
    fn pull_needs_a_branch_that_tracks_a_remote_one() {
        let fx = Fixture::new();
        let path = fx.create_repo("local");
        commit(&path, "a.txt", "a\n", "first");
        assert_error(
            &failed_pull(&fx, "local"),
            Error::NoUpstream("main doesn't track a remote branch.".into()),
        );

        git(&path, ["checkout", "-q", "--detach"]);
        assert_error(&failed_pull(&fx, "local"), Error::NoUpstream(no_branch()));
    }

    /// A clone with `feature`, tracking the upstream's, checked out on `main`.
    fn feature_clone(fx: &Fixture, name: &str) -> (PathBuf, PathBuf) {
        let (upstream, path) = fx.create_clone(name);
        git(&upstream, ["branch", "feature"]);
        git(&path, ["fetch", "-q"]);
        git(
            &path,
            ["branch", "-q", "--track", "feature", "origin/feature"],
        );
        (upstream, path)
    }

    /// Pulls `branch` into the repository `name`.
    fn pull_branch_of(fx: &Fixture, name: &str, branch: &str) -> Result<()> {
        smol::block_on(async {
            let repo = fx.repos.open(name).await.unwrap();
            let pulled = pull_branch(&repo, &valid(branch)).await;
            fetches_done(&repo).await;
            pulled
        })
    }

    #[test]
    fn pull_branch_fast_forwards_a_branch_that_isnt_checked_out_leaving_the_working_tree() {
        let fx = Fixture::new();
        let (upstream, path) = feature_clone(&fx, "ff-other");
        git(&upstream, ["checkout", "-q", "feature"]);
        commit(&upstream, "a.txt", "a\nb\n", "second");
        fs::write(path.join("a.txt"), "uncommitted\n").unwrap();
        pull_branch_of(&fx, "ff-other", "feature").unwrap();
        assert_eq!(subjects(&path), ["first"]);
        assert_eq!(
            lines(&git(&path, ["log", "--format=%s", "feature"])),
            ["second", "first"]
        );
        assert_eq!(
            git(&path, ["rev-parse", "feature"]),
            git(&path, ["rev-parse", "origin/feature"])
        );
        assert_eq!(
            fs::read_to_string(path.join("a.txt")).unwrap(),
            "uncommitted\n"
        );
    }

    #[test]
    fn pull_branch_pulls_the_checked_out_branch_as_pull_does() {
        let fx = Fixture::new();
        let (upstream, path) = fx.create_clone("ff-current");
        commit(&upstream, "a.txt", "a\nb\n", "second");
        pull_branch_of(&fx, "ff-current", "main").unwrap();
        assert_eq!(subjects(&path), ["second", "first"]);
        assert_eq!(fs::read_to_string(path.join("a.txt")).unwrap(), "a\nb\n");
    }

    #[test]
    fn pull_branch_leaves_a_branch_thats_ahead_of_its_upstream_as_it_is() {
        let fx = Fixture::new();
        let (_, path) = feature_clone(&fx, "ff-ahead");
        git(&path, ["checkout", "-q", "feature"]);
        commit(&path, "b.txt", "b\n", "ours");
        git(&path, ["checkout", "-q", "main"]);
        let before = git(&path, ["rev-parse", "feature"]);
        pull_branch_of(&fx, "ff-ahead", "feature").unwrap();
        assert_eq!(git(&path, ["rev-parse", "feature"]), before);
    }

    #[test]
    fn pull_branch_refuses_a_branch_that_diverged_from_its_upstream() {
        let fx = Fixture::new();
        let (upstream, path) = feature_clone(&fx, "ff-diverged");
        git(&upstream, ["checkout", "-q", "feature"]);
        commit(&upstream, "theirs.txt", "theirs\n", "theirs");
        git(&path, ["checkout", "-q", "feature"]);
        commit(&path, "ours.txt", "ours\n", "ours");
        git(&path, ["checkout", "-q", "main"]);
        let before = git(&path, ["rev-parse", "feature"]);
        assert_error(
            &pull_branch_of(&fx, "ff-diverged", "feature").unwrap_err(),
            Error::PullBlocked(
                "feature and origin/feature have diverged. Switch to feature to pull it.".into(),
            ),
        );
        assert_eq!(git(&path, ["rev-parse", "feature"]), before);
        // Fetched all the same.
        assert_eq!(
            git(&path, ["log", "-1", "--format=%s", "origin/feature"]),
            "theirs"
        );
    }

    #[test]
    fn pull_branch_refuses_a_branch_checked_out_in_another_worktree() {
        let fx = Fixture::new();
        let (upstream, path) = feature_clone(&fx, "ff-worktree");
        git(&upstream, ["checkout", "-q", "feature"]);
        commit(&upstream, "a.txt", "a\nb\n", "second");
        let worktree = fx.root().join("ff-worktree-feature");
        git(
            &path,
            [
                "worktree",
                "add",
                "-q",
                worktree.to_str().unwrap(),
                "feature",
            ],
        );
        let before = git(&path, ["rev-parse", "feature"]);
        assert_error(
            &pull_branch_of(&fx, "ff-worktree", "feature").unwrap_err(),
            Error::PullBlocked(format!(
                "feature is checked out in {}, so it wasn't pulled. Switch to another branch there first.",
                worktree.display()
            )),
        );
        assert_eq!(git(&path, ["rev-parse", "feature"]), before);
    }

    #[test]
    fn pull_branch_needs_a_branch_that_tracks_another() {
        let fx = Fixture::new();
        let (_, path) = fx.create_clone("ff-untracked");
        git(&path, ["branch", "lonely"]);
        assert_error(
            &pull_branch_of(&fx, "ff-untracked", "lonely").unwrap_err(),
            Error::NoUpstream("lonely doesn't track a remote branch.".into()),
        );
    }

    #[test]
    fn pull_branch_fast_forwards_to_a_local_upstream() {
        let fx = Fixture::new();
        let (_, path) = fx.create_clone("ff-local");
        git(&path, ["branch", "-q", "--track", "feature", "main"]);
        commit(&path, "b.txt", "b\n", "second");
        pull_branch_of(&fx, "ff-local", "feature").unwrap();
        assert_eq!(
            git(&path, ["rev-parse", "feature"]),
            git(&path, ["rev-parse", "main"])
        );
    }

    #[test]
    fn pull_branch_needs_a_branchs_name() {
        // Refused as the input's parsed: `pull_branch` takes a `BranchShortName`.
        let error = BranchShortName::parse("").unwrap_err();
        assert!(matches!(error, Error::InvalidInput(_)), "{error:?}");
    }
}
