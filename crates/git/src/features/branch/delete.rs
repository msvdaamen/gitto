//! Deleting a branch, local or remote, with the local branches tracking a remote one. Ported from
//! `delete.ts`.

use futures::future::BoxFuture;

use super::commands::{branch_name, tracking_locals};
use super::in_use::branches_in_use;
use crate::core::errors::{Error, Result};
use crate::core::repo::{GitCommand, Repo};
use crate::features::refs::schema::RefKind;
use crate::features::remote::commands::list_remotes;
use crate::features::remote::remote_name::remote_of;
use crate::input::BranchRef;

/// Deletes the branch `ref` (a full ref name), with the commits only it has. A local branch is
/// deleted here only; git refuses one that's checked out, or being rebased or bisected, in any
/// worktree, saying so.
///
/// A remote branch is deleted on its remote, the branch there found as the remote's fetch settings
/// map it to `ref`, then the local branches that track it are; or, if the remote's gone, only what's
/// left of it here. Nothing is deleted if git wouldn't delete one of those local branches
/// (`Error::BranchDeletionBlocked`), nor if the remote's settings don't fetch `ref` from a branch
/// there. Should a local branch still fail to be deleted after the remote one was, checked out in a
/// terminal meanwhile say, that's returned, saying the remote one is gone.
///
/// Its place in line among the writes is taken when it's called, not when it's first polled, as
/// in TS: a write asked for after it waits for it.
pub fn delete_branch(repo: &Repo, reference: &BranchRef) -> BoxFuture<'static, Result<()>> {
    let reference = reference.as_str();
    let Some(target) = branch_name(reference) else {
        let error = Error::Bug(format!("{reference} isn't a branch."));
        return Box::pin(std::future::ready(Err(error)));
    };
    if target.kind == RefKind::Local {
        let write = repo.write(["branch", "--delete", "--force", "--", target.name.as_str()]);
        return Box::pin(async move {
            write.await?;
            Ok(())
        });
    }

    let reference = reference.to_owned();
    let fetcher = repo.clone();
    // One write, from finding the local branches tracking it to deleting them, so no other write
    // (a switch, say) can check one out in between, once the remote one is gone. That holds up the
    // other writes while the remote answers, which a deletion, asked for and waited on, can afford.
    Box::pin(repo.exclusive(move |run| async move {
        let (tracking, in_use, fetched, remotes) = futures::try_join!(
            tracking_locals(&run, &reference),
            branches_in_use(&run),
            fetched_from(&run, &reference),
            list_remotes(&run),
        )?;
        for local in &tracking {
            if let Some(using) = in_use.get(&format!("refs/heads/{}", local.name)) {
                return Err(Error::BranchDeletionBlocked(format!(
                    "{} tracks {} and {}, so neither was deleted. {}",
                    local.name, target.name, using.state, using.fix
                )));
            }
        }

        if let Some(fetched) = &fetched {
            // As a full ref name, which the remote deletes even if it's gone there already: a short
            // one that's no longer there fails. Pushing also drops the remote-tracking branch here.
            // In turn with the fetches too, which change the remote-tracking branches as well, and
            // stopped if Gitto exits, as they are. The pre-push hook runs, as for any push: it's
            // the user's say what may be pushed.
            fetcher
                .fetch([
                    "push",
                    "--quiet",
                    "--delete",
                    "--",
                    fetched.remote.as_str(),
                    fetched.branch.as_str(),
                ])
                .await
                .map_err(Error::without_hints)?;
        } else {
            if let Some(remote) = remote_of(&remotes, &target.name) {
                return Err(Error::BranchDeletionBlocked(format!(
                    "{remote}'s fetch settings don't fetch {} from a branch there, so nothing was deleted.",
                    target.name
                )));
            }
            run.run([
                "branch",
                "--delete",
                "--remotes",
                "--force",
                "--",
                target.name.as_str(),
            ])
            .await?;
        }
        if tracking.is_empty() {
            return Ok(());
        }
        let mut args = vec!["branch", "--delete", "--force", "--"];
        args.extend(tracking.iter().map(|local| local.name.as_str()));
        match (run.run(args).await, &fetched) {
            (Ok(_), _) => Ok(()),
            (Err(Error::Git(error)), Some(fetched)) => Err(Error::Git(error.with_message(format!(
                "{} was deleted on {}, but not every local branch tracking it was. {}",
                target.name, fetched.remote, error.message
            )))),
            (Err(error), _) => Err(error),
        }
    }))
}

/// A remote, and the branch on it that fetching updates a remote-tracking branch from (see
/// `fetched_from`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Fetched {
    remote: String,
    branch: String,
}

/// The remote, and the branch on it, that fetching updates the remote-tracking branch `ref` (a full
/// ref name) from, as the remotes' fetch refspecs say; `None` if none of them does.
async fn fetched_from(run: &GitCommand, reference: &str) -> Result<Option<Fetched>> {
    // Each setting as its key, a newline, and its value, NUL-terminated; exits with 1 if there are
    // none.
    let settings = match run
        .run(["config", "-z", "--get-regexp", r"^remote\..*\.fetch$"])
        .await
    {
        Ok(settings) => settings,
        Err(error) if error.is_exit(1) => String::new(),
        Err(error) => return Err(error),
    };
    for setting in settings.split('\0') {
        let Some(newline) = setting.find('\n') else {
            continue;
        };
        if let Some(branch) = fetched_into(&setting[newline + 1..], reference) {
            let key = &setting[..newline];
            let remote = key
                .strip_prefix("remote.")
                .and_then(|key| key.strip_suffix(".fetch"))
                .unwrap_or(key);
            return Ok(Some(Fetched {
                remote: remote.to_owned(),
                branch,
            }));
        }
    }
    Ok(None)
}

/// The ref the fetch refspec `spec` fetches into `ref`, e.g. `refs/heads/main` for
/// `refs/remotes/origin/main` with `+refs/heads/*:refs/remotes/origin/*`; `None` if it doesn't
/// fetch into `ref`, or only says what not to fetch (`^refs/heads/x`).
pub fn fetched_into(spec: &str, reference: &str) -> Option<String> {
    if spec.starts_with('^') {
        return None;
    }
    let mut sides = spec.strip_prefix('+').unwrap_or(spec).split(':');
    let (source, destination) = (sides.next()?, sides.next()?);
    if source.is_empty() || destination.is_empty() {
        return None;
    }
    let Some(star) = destination.find('*') else {
        return (destination == reference).then(|| source.to_owned());
    };
    let (before, after) = (&destination[..star], &destination[star + 1..]);
    if reference.len() <= before.len() + after.len()
        || !reference.starts_with(before)
        || !reference.ends_with(after)
    {
        return None;
    }
    Some(source.replacen(
        '*',
        &reference[before.len()..reference.len() - after.len()],
        1,
    ))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use regex::Regex;

    use super::*;
    use crate::test_support::{Fixture, git, valid};

    /// The full names of the refs under `prefix` in the repository at `path`.
    fn refs(path: &Path, prefix: &str) -> Vec<String> {
        git(path, ["for-each-ref", "--format=%(refname)", prefix])
            .split('\n')
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect()
    }

    /// Commits `file` with `contents` in the repository at `path`.
    fn commit(path: &Path, file: &str, contents: &str) {
        fs::write(path.join(file), contents).unwrap();
        git(path, ["add", "."]);
        git(path, ["commit", "-qm", file]);
    }

    /// A repository `name` on `main`, with an `other` branch a commit ahead of it.
    fn create_branched_repo(fx: &Fixture, name: &str) -> PathBuf {
        let path = fx.create_repo(name);
        commit(&path, "a.txt", "a\n");
        git(&path, ["switch", "-qc", "other"]);
        commit(&path, "other.txt", "other\n");
        git(&path, ["switch", "-q", "main"]);
        path
    }

    /// A clone `name` of a branched repository, with a local `other` tracking `origin/other`; the
    /// repository it's cloned from, which plays the remote, and the clone.
    fn create_tracking_clone(fx: &Fixture, name: &str) -> (PathBuf, PathBuf) {
        let origin = create_branched_repo(fx, &format!("{name}-origin"));
        let path = fx.clone_repo(name, &origin);
        git(&path, ["branch", "-q", "--track", "other", "origin/other"]);
        (origin, path)
    }

    fn strs(refs: &[&str]) -> Vec<String> {
        refs.iter().map(|name| (*name).to_owned()).collect()
    }

    // Deleting a branch.

    #[test]
    fn deletes_a_local_branch_here_only_keeping_the_remote_one() {
        let fx = Fixture::new();
        let (origin, path) = create_tracking_clone(&fx, "delete-local");

        smol::block_on(async {
            let repo = fx.repos.open("delete-local").await.unwrap();
            delete_branch(&repo, &valid("refs/heads/other"))
                .await
                .unwrap();
        });
        assert_eq!(refs(&path, "refs/heads/"), strs(&["refs/heads/main"]));
        assert!(refs(&path, "refs/remotes/origin/").contains(&"refs/remotes/origin/other".into()));
        assert!(refs(&origin, "refs/heads/").contains(&"refs/heads/other".into()));
        let config = git(&path, ["config", "--get-regexp", r"^branch\."]);
        assert!(
            !Regex::new(r"(?m)^branch\.other\.")
                .unwrap()
                .is_match(&config),
            "{config}"
        );
    }

    #[test]
    fn deletes_a_local_branch_with_commits_no_other_branch_has() {
        let fx = Fixture::new();
        let path = create_branched_repo(&fx, "delete-unmerged");

        smol::block_on(async {
            let repo = fx.repos.open("delete-unmerged").await.unwrap();
            delete_branch(&repo, &valid("refs/heads/other"))
                .await
                .unwrap();
        });
        assert_eq!(refs(&path, "refs/heads/"), strs(&["refs/heads/main"]));
    }

    #[test]
    fn refuses_to_delete_the_local_branch_checked_out_here_or_in_another_worktree() {
        let fx = Fixture::new();
        let path = create_branched_repo(&fx, "delete-checked-out");
        let linked = fx.root().join("delete-checked-out-linked");
        git(
            &path,
            ["worktree", "add", "-q", linked.to_str().unwrap(), "other"],
        );
        smol::block_on(async {
            let repo = fx.repos.open("delete-checked-out").await.unwrap();

            let error = delete_branch(&repo, &valid("refs/heads/main"))
                .await
                .unwrap_err();
            assert!(
                Regex::new("cannot delete branch 'main' used by worktree")
                    .unwrap()
                    .is_match(&error.to_string()),
                "{error}"
            );
            let error = delete_branch(&repo, &valid("refs/heads/other"))
                .await
                .unwrap_err();
            assert!(
                Regex::new("cannot delete branch 'other' used by worktree")
                    .unwrap()
                    .is_match(&error.to_string()),
                "{error}"
            );
        });
        assert_eq!(
            refs(&path, "refs/heads/"),
            strs(&["refs/heads/main", "refs/heads/other"])
        );
    }

    #[test]
    fn deletes_a_remote_branch_on_its_remote_and_the_local_branches_tracking_it() {
        let fx = Fixture::new();
        let (origin, path) = create_tracking_clone(&fx, "delete-remote");
        git(
            &path,
            ["branch", "-q", "--track", "also-other", "origin/other"],
        );
        git(
            &path,
            ["branch", "-q", "--no-track", "unrelated", "origin/other"],
        );

        smol::block_on(async {
            let repo = fx.repos.open("delete-remote").await.unwrap();
            delete_branch(&repo, &valid("refs/remotes/origin/other"))
                .await
                .unwrap();
        });
        assert_eq!(refs(&origin, "refs/heads/"), strs(&["refs/heads/main"]));
        assert!(!refs(&path, "refs/remotes/origin/").contains(&"refs/remotes/origin/other".into()));
        assert_eq!(
            refs(&path, "refs/heads/"),
            strs(&["refs/heads/main", "refs/heads/unrelated"])
        );
    }

    #[test]
    fn deletes_a_remote_branch_with_a_slash_in_its_remotes_name() {
        let fx = Fixture::new();
        let origin = create_branched_repo(&fx, "delete-slashed-origin");
        let path = fx.create_repo("delete-slashed");
        git(
            &path,
            ["remote", "add", "team/origin", origin.to_str().unwrap()],
        );
        git(&path, ["fetch", "-q", "team/origin"]);

        smol::block_on(async {
            let repo = fx.repos.open("delete-slashed").await.unwrap();
            delete_branch(&repo, &valid("refs/remotes/team/origin/other"))
                .await
                .unwrap();
        });
        assert_eq!(refs(&origin, "refs/heads/"), strs(&["refs/heads/main"]));
        // Without the remote's HEAD, which newer git also records on a first fetch.
        let remotes: Vec<String> = refs(&path, "refs/remotes/")
            .into_iter()
            .filter(|name| !name.ends_with("/HEAD"))
            .collect();
        assert_eq!(remotes, strs(&["refs/remotes/team/origin/main"]));
    }

    #[test]
    fn deletes_the_branch_on_the_remote_that_its_fetch_settings_fetch_it_from() {
        let fx = Fixture::new();
        let origin = create_branched_repo(&fx, "delete-mapped-origin");
        git(&origin, ["branch", "team/other", "other"]);
        let path = fx.create_repo("delete-mapped");
        git(&path, ["remote", "add", "origin", origin.to_str().unwrap()]);
        git(
            &path,
            [
                "config",
                "remote.origin.fetch",
                "+refs/heads/team/*:refs/remotes/origin/*",
            ],
        );
        git(&path, ["fetch", "-q", "origin"]);

        smol::block_on(async {
            let repo = fx.repos.open("delete-mapped").await.unwrap();
            delete_branch(&repo, &valid("refs/remotes/origin/other"))
                .await
                .unwrap();
        });
        // `team/other`, not `other`, which has the same name here.
        assert_eq!(
            refs(&origin, "refs/heads/"),
            strs(&["refs/heads/main", "refs/heads/other"])
        );
        assert_eq!(refs(&path, "refs/remotes/origin/"), Vec::<String>::new());
    }

    #[test]
    fn deletes_nothing_when_its_remotes_fetch_settings_dont_fetch_it_from_a_branch_there() {
        let fx = Fixture::new();
        let (origin, path) = create_tracking_clone(&fx, "delete-unmapped");
        git(
            &path,
            [
                "config",
                "remote.origin.fetch",
                "+refs/heads/main:refs/remotes/origin/main",
            ],
        );

        smol::block_on(async {
            let repo = fx.repos.open("delete-unmapped").await.unwrap();
            let error = delete_branch(&repo, &valid("refs/remotes/origin/other"))
                .await
                .unwrap_err();
            assert!(
                matches!(error, Error::BranchDeletionBlocked(_)),
                "{error:?}"
            );
            assert_eq!(
                error.to_string(),
                "origin's fetch settings don't fetch origin/other from a branch there, so nothing was deleted."
            );
        });
        assert!(refs(&origin, "refs/heads/").contains(&"refs/heads/other".into()));
        assert!(refs(&path, "refs/heads/").contains(&"refs/heads/other".into()));
    }

    #[test]
    fn deletes_nothing_when_a_local_branch_tracking_the_remote_one_is_checked_out() {
        let fx = Fixture::new();
        let (origin, path) = create_tracking_clone(&fx, "delete-remote-checked-out");
        let linked = fx.root().join("delete-remote-checked-out-linked");
        git(
            &path,
            ["worktree", "add", "-q", linked.to_str().unwrap(), "other"],
        );

        smol::block_on(async {
            let repo = fx.repos.open("delete-remote-checked-out").await.unwrap();
            let error = delete_branch(&repo, &valid("refs/remotes/origin/other"))
                .await
                .unwrap_err();
            assert!(
                matches!(error, Error::BranchDeletionBlocked(_)),
                "{error:?}"
            );
            assert!(
                Regex::new(
                    r"^other tracks origin/other and is checked out in .*delete-remote-checked-out-linked, so neither was deleted\. Switch to another branch there first\.$"
                )
                .unwrap()
                .is_match(&error.to_string()),
                "{error}"
            );
        });
        assert!(refs(&origin, "refs/heads/").contains(&"refs/heads/other".into()));
        assert!(refs(&path, "refs/remotes/origin/").contains(&"refs/remotes/origin/other".into()));
        assert!(refs(&path, "refs/heads/").contains(&"refs/heads/other".into()));
    }

    #[test]
    fn holds_up_the_other_writes_until_its_done_so_none_checks_out_a_tracking_branch_meanwhile() {
        let fx = Fixture::new();
        let (origin, path) = create_tracking_clone(&fx, "delete-in-turn");
        smol::block_on(async {
            let repo = fx.repos.open("delete-in-turn").await.unwrap();

            let deleting = delete_branch(&repo, &valid("refs/remotes/origin/other"));
            let switching = repo.write(["switch", "other"]);
            deleting.await.unwrap();
            let error = switching.await.unwrap_err();
            assert!(
                error.to_string().contains("invalid reference: other"),
                "{error}"
            );
        });
        assert_eq!(refs(&origin, "refs/heads/"), strs(&["refs/heads/main"]));
        assert_eq!(refs(&path, "refs/heads/"), strs(&["refs/heads/main"]));
    }

    #[test]
    fn deletes_nothing_when_a_local_branch_tracking_the_remote_one_is_being_rebased() {
        let fx = Fixture::new();
        let (origin, path) = create_tracking_clone(&fx, "delete-remote-rebasing");
        let linked = fx.root().join("delete-remote-rebasing-linked");
        git(
            &path,
            ["worktree", "add", "-q", linked.to_str().unwrap(), "other"],
        );
        commit(&path, "main.txt", "main\n");
        // Stops at its first commit, HEAD detached, as an `exec` that fails does.
        assert_ne!(
            crate::test_support::conflicts::try_git(
                &linked,
                ["rebase", "-q", "--exec", "false", "main"]
            ),
            0
        );

        smol::block_on(async {
            let repo = fx.repos.open("delete-remote-rebasing").await.unwrap();
            let error = delete_branch(&repo, &valid("refs/remotes/origin/other"))
                .await
                .unwrap_err();
            assert!(
                error.to_string().contains(
                    "other tracks origin/other and is being rebased, so neither was deleted. Finish or abort the rebase first."
                ),
                "{error}"
            );
        });
        assert!(refs(&origin, "refs/heads/").contains(&"refs/heads/other".into()));
        assert!(refs(&path, "refs/heads/").contains(&"refs/heads/other".into()));
    }

    #[test]
    fn deletes_nothing_when_a_local_branch_tracking_the_remote_one_is_being_bisected() {
        let fx = Fixture::new();
        let (origin, path) = create_tracking_clone(&fx, "delete-remote-bisecting");
        git(&path, ["switch", "-q", "other"]);
        git(&path, ["bisect", "start"]);
        git(&path, ["switch", "-q", "--detach", "main"]);

        smol::block_on(async {
            let repo = fx.repos.open("delete-remote-bisecting").await.unwrap();
            let error = delete_branch(&repo, &valid("refs/remotes/origin/other"))
                .await
                .unwrap_err();
            assert!(
                error.to_string().contains(
                    "other tracks origin/other and is being bisected, so neither was deleted. Stop bisecting first."
                ),
                "{error}"
            );
        });
        assert!(refs(&origin, "refs/heads/").contains(&"refs/heads/other".into()));
        assert!(refs(&path, "refs/heads/").contains(&"refs/heads/other".into()));
    }

    #[test]
    fn says_the_remote_branch_is_gone_when_a_local_one_tracking_it_couldnt_be_deleted_after() {
        let fx = Fixture::new();
        let (origin, path) = create_tracking_clone(&fx, "delete-local-fails");
        // As if another git process were writing the branch.
        fs::write(
            path.join(".git")
                .join("refs")
                .join("heads")
                .join("other.lock"),
            "",
        )
        .unwrap();

        smol::block_on(async {
            let repo = fx.repos.open("delete-local-fails").await.unwrap();
            let error = delete_branch(&repo, &valid("refs/remotes/origin/other"))
                .await
                .unwrap_err();
            assert!(
                error.to_string().starts_with(
                    "origin/other was deleted on origin, but not every local branch tracking it was. "
                ),
                "{error}"
            );
        });
        assert_eq!(refs(&origin, "refs/heads/"), strs(&["refs/heads/main"]));
    }

    #[test]
    fn deletes_whats_left_here_of_a_branch_deleted_on_the_remote_already() {
        let fx = Fixture::new();
        let (origin, path) = create_tracking_clone(&fx, "delete-remote-gone");
        git(&origin, ["branch", "-qD", "other"]);

        smol::block_on(async {
            let repo = fx.repos.open("delete-remote-gone").await.unwrap();
            delete_branch(&repo, &valid("refs/remotes/origin/other"))
                .await
                .unwrap();
        });
        assert!(!refs(&path, "refs/remotes/origin/").contains(&"refs/remotes/origin/other".into()));
        assert_eq!(refs(&path, "refs/heads/"), strs(&["refs/heads/main"]));
    }

    #[test]
    fn deletes_the_branch_left_here_of_a_remote_thats_gone() {
        let fx = Fixture::new();
        let (_, path) = create_tracking_clone(&fx, "delete-remote-removed");
        git(
            &path,
            ["update-ref", "refs/remotes/old/other", "origin/other"],
        );

        smol::block_on(async {
            let repo = fx.repos.open("delete-remote-removed").await.unwrap();
            delete_branch(&repo, &valid("refs/remotes/old/other"))
                .await
                .unwrap();
        });
        assert_eq!(refs(&path, "refs/remotes/old/"), Vec::<String>::new());
        assert_eq!(
            refs(&path, "refs/heads/"),
            strs(&["refs/heads/main", "refs/heads/other"])
        );
    }

    #[test]
    fn says_why_the_remote_wouldnt_delete_it_deleting_nothing_here() {
        let fx = Fixture::new();
        let (_, path) = create_tracking_clone(&fx, "delete-remote-refused");
        let gone = fx.root().join("gone");
        git(
            &path,
            ["remote", "set-url", "origin", gone.to_str().unwrap()],
        );

        smol::block_on(async {
            let repo = fx.repos.open("delete-remote-refused").await.unwrap();
            let error = delete_branch(&repo, &valid("refs/remotes/origin/other"))
                .await
                .unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("does not appear to be a git repository"),
                "{error}"
            );
        });
        assert!(refs(&path, "refs/remotes/origin/").contains(&"refs/remotes/origin/other".into()));
        assert!(refs(&path, "refs/heads/").contains(&"refs/heads/other".into()));
    }

    #[test]
    fn refuses_to_delete_what_isnt_a_branch() {
        let fx = Fixture::new();
        let path = create_branched_repo(&fx, "delete-other-refs");
        git(&path, ["tag", "v1"]);
        smol::block_on(async {
            let repo = fx.repos.open("delete-other-refs").await.unwrap();

            // A tag is refused as the input's parsed: `delete_branch` takes a `BranchRef`.
            assert!(BranchRef::parse("refs/tags/v1").is_err());
            let error = delete_branch(&repo, &valid("refs/remotes/origin/HEAD"))
                .await
                .unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("refs/remotes/origin/HEAD isn't a branch."),
                "{error}"
            );
        });
        assert_eq!(refs(&path, "refs/tags/"), strs(&["refs/tags/v1"]));
    }

    // Where a fetch refspec fetches a remote-tracking branch from.

    #[test]
    fn maps_it_back_through_the_pattern() {
        let spec = "+refs/heads/*:refs/remotes/origin/*";
        assert_eq!(
            fetched_into(spec, "refs/remotes/origin/feature/x").as_deref(),
            Some("refs/heads/feature/x")
        );
        assert_eq!(
            fetched_into(
                "refs/heads/team/*:refs/remotes/origin/*",
                "refs/remotes/origin/x"
            )
            .as_deref(),
            Some("refs/heads/team/x")
        );
        assert_eq!(
            fetched_into(
                "refs/heads/*-dev:refs/remotes/origin/*-dev",
                "refs/remotes/origin/a-dev"
            )
            .as_deref(),
            Some("refs/heads/a-dev")
        );
    }

    #[test]
    fn maps_an_exact_one() {
        assert_eq!(
            fetched_into(
                "refs/heads/main:refs/remotes/origin/main",
                "refs/remotes/origin/main"
            )
            .as_deref(),
            Some("refs/heads/main")
        );
        assert_eq!(
            fetched_into(
                "refs/heads/main:refs/remotes/origin/main",
                "refs/remotes/origin/x"
            ),
            None
        );
    }

    #[test]
    fn maps_nothing_it_doesnt_fetch_into() {
        assert_eq!(
            fetched_into(
                "+refs/heads/*:refs/remotes/origin/*",
                "refs/remotes/other/x"
            ),
            None
        );
        assert_eq!(
            fetched_into(
                "+refs/heads/*:refs/remotes/origin/*",
                "refs/remotes/origin/"
            ),
            None
        );
        assert_eq!(fetched_into("^refs/heads/x", "refs/remotes/origin/x"), None);
        assert_eq!(
            fetched_into("refs/heads/main", "refs/remotes/origin/main"),
            None
        );
    }
}
