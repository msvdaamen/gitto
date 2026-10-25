//! Pushing the checked-out branch, and where the Push button pushes it.

use std::collections::HashSet;

use super::blockers::Action;
use super::config::{map_ref, push_remote, read_config, remote_names};
use super::progress::why_detached;
use super::schema::PushTarget;
use crate::core::errors::{Error, Result};
use crate::core::git_dirs::git_dirs;
use crate::core::repo::{GitCommand, Repo, current_branch, resolve_ref};
use crate::core::runner::RunOptions;

/// Where the checked-out branch is pushed (see `plan_push`).
#[derive(Debug, Clone)]
struct PushPlan {
    branch: String,
    /// A remote's name, or a URL.
    remote: String,
    /// Whether `remote` is a remote's name, rather than a URL.
    named: bool,
    /// The ref it's pushed to on the remote, e.g. `refs/heads/feature`.
    reference: String,
    /// Its remote-tracking branch, e.g. `refs/remotes/origin/feature`, which may not exist yet;
    /// `None` if the remote's fetch refspecs don't fetch it into one.
    tracking: Option<String>,
    /// Whether `reference` on `remote` is also what the branch pulls from.
    pulls_from: bool,
    /// Whether the branch tracks nothing yet, and so tracks what it's pushed to once it is.
    set_upstream: bool,
}

/// Where the checked-out branch is pushed: to its remote (see `push_remote`), where the remote's
/// push refspecs say if it has any, and otherwise under the same name, as `git push` does by
/// default; or under its upstream's name if `push.default` says to push to the upstream, and
/// that's on the same remote. Only ever the one branch, whatever `push.default` says: the Push
/// button names one. Fails with why it can't be pushed (`Error::PushBlocked`). The repository's
/// git directory is only asked for with HEAD detached.
async fn plan_push(repo: &Repo) -> Result<PushPlan> {
    let run = repo.git();
    let (branch, config, head) = futures::try_join!(
        current_branch(&run),
        read_config(&run),
        resolve_ref(&run, "HEAD"),
    )?;
    let Some(branch) = branch else {
        let git_dir = git_dirs(repo).await?.git_dir;
        return Err(Error::PushBlocked(
            why_detached(&git_dir, Action::Push).await,
        ));
    };
    if head.is_none() {
        return Err(Error::PushBlocked(format!(
            "{branch} has no commits to push yet."
        )));
    }
    let Some(remote) = push_remote(&config, &branch) else {
        return Err(Error::PushBlocked(
            "There's no remote to push to.".to_owned(),
        ));
    };

    let local = format!("refs/heads/{branch}");
    let upstream_remote = config.get(&format!("branch.{branch}.remote"));
    let merge = config.get(&format!("branch.{branch}.merge"));
    let push_default = config.get("push.default").map(str::to_lowercase);
    let push_specs = config.all(&format!("remote.{remote}.push"));
    let reference = if !push_specs.is_empty() {
        match map_ref(push_specs, &local, true) {
            Some(reference) => reference,
            None => {
                return Err(Error::PushBlocked(format!(
                    "{remote}'s push settings don't push {branch} anywhere."
                )));
            }
        }
    } else if let Some(merge) = merge.filter(|merge| {
        merge.starts_with("refs/heads/")
            && upstream_remote == Some(remote.as_str())
            && matches!(push_default.as_deref(), Some("upstream" | "tracking"))
    }) {
        merge.to_owned()
    } else {
        local
    };
    // A URL, rather than a remote's name, has no settings, so no remote-tracking branches.
    let named = remote_names(&config, &["url", "pushurl"]).contains(&remote);
    let tracking = if named {
        map_ref(
            config.all(&format!("remote.{remote}.fetch")),
            &reference,
            false,
        )
    } else {
        None
    };
    Ok(PushPlan {
        pulls_from: upstream_remote == Some(remote.as_str()) && merge == Some(reference.as_str()),
        set_upstream: upstream_remote.is_none_or(str::is_empty) || merge.is_none_or(str::is_empty),
        branch,
        remote,
        named,
        reference,
        tracking,
    })
}

/// What to call the ref `plan` pushes to, e.g. `origin/feature`.
fn target_name(plan: &PushPlan) -> String {
    let reference = plan
        .reference
        .strip_prefix("refs/heads/")
        .unwrap_or(&plan.reference);
    format!("{}/{reference}", plan.remote)
}

/// Where the Push button pushes the checked-out branch, and how many commits it sends, or why not.
pub async fn get_push_target(repo: &Repo) -> Result<PushTarget> {
    let plan = match plan_push(repo).await {
        Ok(plan) => plan,
        Err(Error::PushBlocked(reason)) => return Ok(PushTarget::Blocked { reason }),
        Err(error) => return Err(error),
    };
    let run = repo.git();
    let tracked = match &plan.tracking {
        Some(tracking) => resolve_ref(&run, tracking).await?.is_some(),
        None => false,
    };
    // What the remote doesn't have: past its branch, or, without that, past all of its branches
    // (all remotes' for a URL, which has none of its own).
    let known = match (&plan.tracking, tracked, plan.named) {
        (Some(tracking), true, _) => tracking.clone(),
        (_, _, true) => format!("--remotes={}", plan.remote),
        _ => "--remotes".to_owned(),
    };
    let ahead = run
        .run(["rev-list", "--count", "HEAD", "--not", &known])
        .await?;
    let ahead = ahead
        .trim()
        .parse()
        .map_err(|_| Error::Bug(format!("git rev-list --count said {ahead:?}")))?;
    Ok(PushTarget::Remote {
        name: target_name(&plan),
        is_new: plan.tracking.is_some() && !tracked,
        ahead,
        pulls_from: plan.pulls_from,
    })
}

/// Pushes the checked-out branch where `plan_push` says; a branch that doesn't track one yet then
/// tracks it.
///
/// With `force`, replaces the remote branch even if it has commits the branch doesn't, as after a
/// rebase or an amend; but only if they're ones the branch had: not if someone pushed others since
/// (`--force-with-lease`), even ones fetched in the background meanwhile (`--force-if-includes`).
/// Fails with an `Error::PushRejected` if the remote branch has commits the branch doesn't, which
/// says whether forcing would get past that.
pub async fn push(repo: &Repo, force: bool) -> Result<()> {
    let plan = plan_push(repo).await?;
    let local = format!("refs/heads/{}", plan.branch);
    let mut args = vec![
        "push".to_owned(),
        // What happened to the ref, on stdout, to tell a rejection from other failures.
        "--porcelain".to_owned(),
    ];
    if force {
        args.push("--force-with-lease".to_owned());
        args.push("--force-if-includes".to_owned());
    }
    args.push("--".to_owned());
    args.push(plan.remote.clone());
    args.push(format!("{local}:{}", plan.reference));
    // Queued with fetches rather than writes: it waits on the network, and only updates the
    // remote-tracking branch, which writes don't touch. Stopped if Gitto exits, as a fetch is:
    // the remote takes a ref's update whole or not at all, and pushing again finishes it.
    let pushed = repo
        .fetching(|run| async move { run.run(&args).await })
        .await;
    if let Err(error) = pushed {
        let Error::Git(failure) = error else {
            return Err(error);
        };
        let rejected = match rejection(&failure.stdout_text(), &local) {
            Some(reason) => rejected_error(&repo.git(), &plan, &reason).await?,
            None => None,
        };
        return Err(rejected.unwrap_or_else(|| Error::Git(failure.without_hints())));
    }
    if plan.set_upstream {
        set_upstream(repo, &plan).await?;
    }
    Ok(())
}

/// Has the branch track what `plan` pushed it to, as `--set-upstream` would, but with the other
/// writes to the config rather than alongside them. Both settings or neither: the remote is set
/// last, and the ref taken back if that fails, as without it git takes the branch to track
/// nothing.
async fn set_upstream(repo: &Repo, plan: &PushPlan) -> Result<()> {
    let merge_key = format!("branch.{}.merge", plan.branch);
    let remote_key = format!("branch.{}.remote", plan.branch);
    let (reference, remote) = (plan.reference.clone(), plan.remote.clone());
    repo.exclusive(|run| async move {
        run.run(["config", &merge_key, &reference]).await?;
        if let Err(error) = run.run(["config", &remote_key, &remote]).await {
            let _ = run.run(["config", "--unset", &merge_key]).await;
            return Err(error);
        }
        Ok(())
    })
    .await
}

/// Why git wouldn't push `reference` for what's on the remote, as `git push --porcelain` says on
/// `stdout`: e.g. "non-fast-forward". `None` if it didn't say: the remote itself may have
/// declined it (`[remote rejected]`), as a hook or `receive.denyNonFastForwards` does, which
/// force-pushing doesn't get past.
pub fn rejection(stdout: &str, reference: &str) -> Option<String> {
    // Each ref on a line of its own: `!`, then `<from>:<to>`, then `[rejected] (<reason>)`, by
    // tabs.
    for line in stdout.split('\n') {
        let mut fields = line.split('\t');
        let flag = fields.next().unwrap_or("");
        let refs = fields.next().unwrap_or("");
        let summary = fields.next().unwrap_or("");
        let reason = summary
            .strip_prefix("[rejected] (")
            .and_then(|rest| rest.strip_suffix(')'));
        if flag == "!"
            && refs.split(':').next() == Some(reference)
            && let Some(reason) = reason
        {
            return Some(reason.to_owned());
        }
    }
    None
}

/// Why pushing as `plan` says was turned down for `reason` (see `rejection`), if that's for the
/// remote branch having commits the branch doesn't; `None` otherwise, which git explains itself.
async fn rejected_error(run: &GitCommand, plan: &PushPlan, reason: &str) -> Result<Option<Error>> {
    let target = target_name(plan);
    let not_pulled = format!("{target} has commits you haven't pulled yet.");
    let rejected =
        |message: String, forcible: bool| Some(Error::PushRejected { message, forcible });
    Ok(match reason {
        // Commits the repository has fetched: ones the branch had, before a rebase or an amend,
        // say, or someone else's.
        "non-fast-forward" => {
            if had_commit(run, &plan.branch, plan.tracking.as_deref()).await? {
                rejected(
                    format!(
                        "{target} still has {}'s commits from before a rebase or amend.",
                        plan.branch
                    ),
                    true,
                )
            } else {
                rejected(not_pulled, false)
            }
        }
        // Commits it hasn't fetched: someone else pushed to it.
        "fetch first" => rejected(not_pulled, false),
        // A force-push that found it changed since it was fetched (`--force-with-lease`), or with
        // commits that were fetched but never in the branch (`--force-if-includes`).
        "stale info" | "remote ref updated since checkout" => {
            let not_pulled = not_pulled.strip_suffix('.').unwrap_or(&not_pulled);
            rejected(format!("{not_pulled}, so it wasn't force-pushed."), false)
        }
        _ => None,
    })
}

/// Whether `branch` ever had the commit `tracking` points at, or one after it: the check
/// `--force-if-includes` makes, so the force-push it's offered for goes through. Not without a
/// remote-tracking branch, which `--force-with-lease` needs too.
async fn had_commit(run: &GitCommand, branch: &str, tracking: Option<&str>) -> Result<bool> {
    let Some(tracking) = tracking else {
        return Ok(false);
    };
    if resolve_ref(run, tracking).await?.is_none() {
        return Ok(false);
    }
    // Every commit the branch has pointed at, by its reflog; none if it keeps none.
    let local = format!("refs/heads/{branch}");
    let reflog = run
        .run(["log", "--walk-reflogs", "--format=%H", &local])
        .await
        .unwrap_or_default();
    let tips: HashSet<&str> = reflog.split('\n').filter(|tip| !tip.is_empty()).collect();
    if tips.is_empty() {
        return Ok(false);
    }
    // The commits in the remote branch's history that none of those has.
    let mut stdin = vec![tracking.to_owned()];
    stdin.extend(tips.iter().map(|tip| format!("^{tip}")));
    let missing = run
        .run_with(
            ["rev-list", "--count", "--stdin"],
            RunOptions::default().stdin(stdin.join("\n") + "\n"),
        )
        .await?;
    Ok(missing.trim().parse::<u64>().ok() == Some(0))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::core::errors::ApiCode;
    use crate::test_support::{Fixture, git};

    fn subjects(path: &Path, rev: &str) -> Vec<String> {
        git(path, ["log", "--format=%s", rev])
            .split('\n')
            .map(str::to_owned)
            .collect()
    }

    fn commit(path: &Path, message: &str) {
        git(path, ["commit", "-q", "--allow-empty", "-m", message]);
    }

    /// Commits to `upstream` from another clone of it, as someone else pushing.
    fn push_from_elsewhere(fx: &Fixture, upstream: &Path, name: &str, message: &str) {
        let other = fx.clone_repo(name, upstream);
        commit(&other, message);
        git(&other, ["push", "-q"]);
    }

    fn remote(target: PushTarget) -> (String, bool, u64, bool) {
        match target {
            PushTarget::Remote {
                name,
                is_new,
                ahead,
                pulls_from,
            } => (name, is_new, ahead, pulls_from),
            blocked => panic!("not a remote: {blocked:?}"),
        }
    }

    fn rejected(error: &Error) -> (&str, bool) {
        match error {
            Error::PushRejected { message, forcible } => (message, *forcible),
            other => panic!("not a PushRejected: {other:?}"),
        }
    }

    #[test]
    fn pushes_the_branch_to_its_upstream() {
        let fx = Fixture::new();
        let (upstream, path) = fx.create_clone("ahead");
        commit(&path, "second");
        smol::block_on(async {
            let repo = fx.repos.open("ahead").await.unwrap();
            assert_eq!(
                get_push_target(&repo).await.unwrap(),
                PushTarget::Remote {
                    name: "origin/main".into(),
                    is_new: false,
                    ahead: 1,
                    pulls_from: true,
                }
            );
            push(&repo, false).await.unwrap();
            assert_eq!(subjects(&upstream, "main"), ["second", "first"]);
            assert_eq!(remote(get_push_target(&repo).await.unwrap()).2, 0);
        });
    }

    #[test]
    fn pushes_a_new_branch_to_its_remote_and_tracks_it_there() {
        let fx = Fixture::new();
        let (upstream, path) = fx.create_clone("new-branch");
        git(&path, ["checkout", "-qb", "feature"]);
        commit(&path, "feature work");
        smol::block_on(async {
            let repo = fx.repos.open("new-branch").await.unwrap();
            assert_eq!(
                get_push_target(&repo).await.unwrap(),
                PushTarget::Remote {
                    name: "origin/feature".into(),
                    is_new: true,
                    ahead: 1,
                    pulls_from: false,
                }
            );
            push(&repo, false).await.unwrap();
            assert_eq!(subjects(&upstream, "feature"), ["feature work", "first"]);
            assert_eq!(
                git(&path, ["rev-parse", "--abbrev-ref", "feature@{upstream}"]),
                "origin/feature"
            );
        });
    }

    #[test]
    fn pushes_a_branch_tracking_another_one_under_its_own_name_leaving_what_it_tracks() {
        let fx = Fixture::new();
        let (upstream, path) = fx.create_clone("other-name");
        git(&path, ["checkout", "-qb", "feature", "origin/main"]);
        commit(&path, "feature work");
        smol::block_on(async {
            let repo = fx.repos.open("other-name").await.unwrap();
            let (name, is_new, _, pulls_from) = remote(get_push_target(&repo).await.unwrap());
            assert_eq!(
                (name.as_str(), is_new, pulls_from),
                ("origin/feature", true, false)
            );
            push(&repo, false).await.unwrap();
            assert_eq!(subjects(&upstream, "feature"), ["feature work", "first"]);
            assert_eq!(subjects(&upstream, "main"), ["first"]);
            assert_eq!(
                git(&path, ["rev-parse", "--abbrev-ref", "feature@{upstream}"]),
                "origin/main"
            );

            // Unless the config says to push to the upstream.
            git(&path, ["config", "push.default", "upstream"]);
            let (name, _, ahead, pulls_from) = remote(get_push_target(&repo).await.unwrap());
            assert_eq!((name.as_str(), ahead, pulls_from), ("origin/main", 1, true));
        });
    }

    #[test]
    fn pushes_where_the_config_says_e_g_to_a_fork() {
        let fx = Fixture::new();
        let (_, path) = fx.create_clone("triangular");
        let (fork, _) = fx.create_clone("triangular-fork");
        git(&path, ["remote", "add", "fork", fork.to_str().unwrap()]);
        git(&path, ["config", "remote.pushDefault", "fork"]);
        commit(&path, "mine");
        smol::block_on(async {
            let repo = fx.repos.open("triangular").await.unwrap();
            let (name, is_new, _, pulls_from) = remote(get_push_target(&repo).await.unwrap());
            assert_eq!(
                (name.as_str(), is_new, pulls_from),
                ("fork/main", true, false)
            );
        });
    }

    #[test]
    fn pushes_where_the_remotes_push_refspecs_say_e_g_for_review() {
        let fx = Fixture::new();
        let (upstream, path) = fx.create_clone("for-review");
        git(
            &path,
            ["config", "remote.origin.push", "refs/heads/*:refs/for/*"],
        );
        commit(&path, "change");
        smol::block_on(async {
            let repo = fx.repos.open("for-review").await.unwrap();
            assert_eq!(
                remote(get_push_target(&repo).await.unwrap()).0,
                "origin/refs/for/main"
            );
            push(&repo, false).await.unwrap();
            assert_eq!(subjects(&upstream, "refs/for/main"), ["change", "first"]);
            assert_eq!(subjects(&upstream, "main"), ["first"]);
        });
    }

    #[test]
    fn pushes_to_a_remote_with_only_a_push_url_or_to_a_url() {
        let fx = Fixture::new();
        let (upstream, path) = fx.create_clone("by-url");
        // A fork: a copy of the upstream, which takes pushes as it does.
        let fork = fx.clone_repo("by-url-fork", &upstream);
        git(
            &fork,
            ["config", "receive.denyCurrentBranch", "updateInstead"],
        );
        git(
            &path,
            ["config", "remote.fork.pushurl", fork.to_str().unwrap()],
        );
        git(&path, ["config", "remote.pushDefault", "fork"]);
        commit(&path, "to the fork");
        smol::block_on(async {
            let repo = fx.repos.open("by-url").await.unwrap();
            assert_eq!(remote(get_push_target(&repo).await.unwrap()).0, "fork/main");
            push(&repo, false).await.unwrap();
            assert_eq!(subjects(&fork, "main"), ["to the fork", "first"]);

            git(
                &path,
                [
                    "config",
                    "branch.main.pushRemote",
                    upstream.to_str().unwrap(),
                ],
            );
            assert!(!remote(get_push_target(&repo).await.unwrap()).1);
            push(&repo, false).await.unwrap();
            assert_eq!(subjects(&upstream, "main"), ["to the fork", "first"]);
        });
    }

    #[test]
    fn finds_the_remote_tracking_branch_through_the_remotes_fetch_refspecs() {
        let fx = Fixture::new();
        let (_, path) = fx.create_clone("refspec");
        git(
            &path,
            [
                "config",
                "--replace-all",
                "remote.origin.fetch",
                "+refs/heads/*:refs/remotes/mirror/*",
            ],
        );
        git(&path, ["fetch", "-q", "--prune"]);
        commit(&path, "second");
        smol::block_on(async {
            let repo = fx.repos.open("refspec").await.unwrap();
            let (name, is_new, ahead, _) = remote(get_push_target(&repo).await.unwrap());
            assert_eq!((name.as_str(), is_new, ahead), ("origin/main", false, 1));
        });
    }

    #[test]
    fn explains_a_rejection_without_a_remote_tracking_branch_to_compare_with() {
        let fx = Fixture::new();
        let (upstream, path) = fx.create_clone("single");
        // Fetches only main, so the branches it pushes have no remote-tracking branches.
        git(
            &path,
            [
                "config",
                "--replace-all",
                "remote.origin.fetch",
                "+refs/heads/main:refs/remotes/origin/main",
            ],
        );
        git(&path, ["checkout", "-qb", "feature"]);
        commit(&path, "feature work");
        smol::block_on(async {
            let repo = fx.repos.open("single").await.unwrap();
            let (name, is_new, _, _) = remote(get_push_target(&repo).await.unwrap());
            assert_eq!((name.as_str(), is_new), ("origin/feature", false));
            push(&repo, false).await.unwrap();
            git(
                &path,
                [
                    "commit",
                    "-q",
                    "--amend",
                    "--allow-empty",
                    "-m",
                    "feature work, reworded",
                ],
            );
            let error = push(&repo, false).await.unwrap_err();
            assert!(!rejected(&error).1);
            assert_eq!(subjects(&upstream, "feature"), ["feature work", "first"]);
        });
    }

    #[test]
    fn pushes_only_the_checked_out_branch_whatever_push_default_says() {
        let fx = Fixture::new();
        let (upstream, path) = fx.create_clone("matching");
        git(&path, ["config", "push.default", "matching"]);
        git(&path, ["branch", "other"]);
        git(&path, ["push", "-q", "origin", "other"]);
        // `other` moves on upstream, so pushing it would be turned down.
        let theirs = git(
            &upstream,
            ["commit-tree", "HEAD^{tree}", "-p", "HEAD", "-m", "theirs"],
        );
        git(&upstream, ["update-ref", "refs/heads/other", &theirs]);
        git(&path, ["fetch", "-q"]);
        commit(&path, "ours");
        smol::block_on(async {
            push(&fx.repos.open("matching").await.unwrap(), false)
                .await
                .unwrap();
        });
        assert_eq!(subjects(&upstream, "main"), ["ours", "first"]);
        assert_eq!(subjects(&upstream, "other"), ["theirs", "first"]);
    }

    #[test]
    fn offers_to_force_push_an_amended_commit_and_replaces_it_when_forced() {
        let fx = Fixture::new();
        let (upstream, path) = fx.create_clone("amended");
        commit(&path, "second");
        git(&path, ["push", "-q"]);
        git(
            &path,
            [
                "commit",
                "-q",
                "--amend",
                "--allow-empty",
                "-m",
                "second, reworded",
            ],
        );
        smol::block_on(async {
            let repo = fx.repos.open("amended").await.unwrap();

            let error = push(&repo, false).await.unwrap_err();
            assert_eq!(
                rejected(&error),
                (
                    "origin/main still has main's commits from before a rebase or amend.",
                    true
                )
            );
            assert_eq!(error.api(), ApiCode::PushRewritten);
            assert_eq!(subjects(&upstream, "main"), ["second", "first"]);

            push(&repo, true).await.unwrap();
            assert_eq!(subjects(&upstream, "main"), ["second, reworded", "first"]);
        });
    }

    #[test]
    fn doesnt_offer_to_force_push_over_someone_elses_commits() {
        let fx = Fixture::new();
        let (upstream, path) = fx.create_clone("behind");
        push_from_elsewhere(&fx, &upstream, "behind-other", "theirs");
        commit(&path, "ours");
        smol::block_on(async {
            let repo = fx.repos.open("behind").await.unwrap();
            let not_pulled = ("origin/main has commits you haven't pulled yet.", false);
            // Not fetched yet.
            let error = push(&repo, false).await.unwrap_err();
            assert_eq!(rejected(&error), not_pulled);
            assert_eq!(error.api(), ApiCode::PushBehind);
            // Fetched, but not pulled.
            git(&path, ["fetch", "-q"]);
            let error = push(&repo, false).await.unwrap_err();
            assert_eq!(rejected(&error), not_pulled);
        });
    }

    #[test]
    fn doesnt_force_push_over_commits_pushed_since_it_last_pulled() {
        let fx = Fixture::new();
        let (upstream, path) = fx.create_clone("raced");
        commit(&path, "second");
        git(&path, ["push", "-q"]);
        git(
            &path,
            [
                "commit",
                "-q",
                "--amend",
                "--allow-empty",
                "-m",
                "second, reworded",
            ],
        );
        push_from_elsewhere(&fx, &upstream, "raced-other", "theirs");
        smol::block_on(async {
            let repo = fx.repos.open("raced").await.unwrap();

            // Not fetched: the remote changed since.
            let stale = push(&repo, true).await.unwrap_err();
            assert_eq!(
                rejected(&stale),
                (
                    "origin/main has commits you haven't pulled yet, so it wasn't force-pushed.",
                    false
                )
            );
            // Fetched in the background, but never pulled into the branch.
            git(&path, ["fetch", "-q"]);
            let unseen = push(&repo, true).await.unwrap_err();
            let (message, forcible) = rejected(&unseen);
            assert!(!forcible);
            assert!(message.contains("wasn't force-pushed"), "{message}");
            assert_eq!(subjects(&upstream, "main"), ["theirs", "second", "first"]);
        });
    }

    #[test]
    fn explains_a_push_the_remote_declined_itself_without_offering_to_force_it() {
        let fx = Fixture::new();
        let (upstream, path) = fx.create_clone("declined");
        git(&upstream, ["config", "receive.denyNonFastForwards", "true"]);
        commit(&path, "second");
        git(&path, ["push", "-q"]);
        git(
            &path,
            [
                "commit",
                "-q",
                "--amend",
                "--allow-empty",
                "-m",
                "second, reworded",
            ],
        );
        smol::block_on(async {
            let error = push(&fx.repos.open("declined").await.unwrap(), true)
                .await
                .unwrap_err();
            assert!(!matches!(error, Error::PushRejected { .. }), "{error:?}");
            assert!(error.to_string().contains("failed to push"), "{error}");
        });
    }

    #[test]
    fn says_why_it_cant_push_without_a_branch_commits_or_a_remote() {
        let fx = Fixture::new();
        let (_, path) = fx.create_clone("blocked");
        let lonely = fx.create_repo("blocked-lonely");
        commit(&lonely, "first");
        smol::block_on(async {
            let repo = fx.repos.open("blocked").await.unwrap();
            git(&path, ["checkout", "-q", "--detach"]);
            assert_eq!(
                get_push_target(&repo).await.unwrap(),
                PushTarget::Blocked {
                    reason: "HEAD is detached: check out a branch to push it.".into()
                }
            );
            let error = push(&repo, false).await.unwrap_err();
            assert!(matches!(error, Error::PushBlocked(_)), "{error:?}");

            git(&path, ["checkout", "-q", "--orphan", "empty"]);
            assert_eq!(
                get_push_target(&repo).await.unwrap(),
                PushTarget::Blocked {
                    reason: "empty has no commits to push yet.".into()
                }
            );

            assert_eq!(
                get_push_target(&fx.repos.open("blocked-lonely").await.unwrap())
                    .await
                    .unwrap(),
                PushTarget::Blocked {
                    reason: "There's no remote to push to.".into()
                }
            );
        });
    }

    #[test]
    fn rejection_reads_why_git_wouldnt_push_the_ref_from_git_push_porcelain() {
        let stdout = [
            "To /remote.git",
            "!\trefs/heads/other:refs/heads/other\t[rejected] (non-fast-forward)",
            "!\trefs/heads/hook:refs/heads/hook\t[remote rejected] (pre-receive hook declined)",
            "!\trefs/heads/main:refs/heads/trunk\t[rejected] (fetch first)",
            "Done",
        ]
        .join("\n");
        assert_eq!(
            rejection(&stdout, "refs/heads/main").as_deref(),
            Some("fetch first")
        );
        // Not another ref's reason, nor the remote's own.
        assert_eq!(rejection(&stdout, "refs/heads/elsewhere"), None);
        assert_eq!(rejection(&stdout, "refs/heads/hook"), None);
    }
}
