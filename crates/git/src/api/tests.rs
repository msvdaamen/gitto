//! The API as the UI uses it, errors and all. Ported from `packages/git/src/router.test.ts`: the
//! Electron app called the features through an oRPC router, which checked the input and the
//! installed git, and turned errors into codes the renderer showed. Here the UI parses its input
//! into the types the features take, and calls them through `GitApi::call`, as these do.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_io::Timer;
use futures::StreamExt;
use futures::future::BoxFuture;

use super::{ApiError, GitApi, pull};
use crate::core::errors::{ApiCode, Error, ListAction, RepositoryListError, Result};
use crate::core::repo::RepositoryLookup;
use crate::core::version::GitVersion;
use crate::features::branch::merge::merge_branch;
use crate::features::branch::schema::MergeOutcome;
use crate::features::diff::commands::get_commit_files;
use crate::features::remote::push::push;
use crate::features::staging::commands::stage;
use crate::features::stash::commands::pop_stash;
use crate::features::status::commands::get_status_since;
use crate::features::status::schema::{Head, StatusReply};
use crate::features::version::check;
use crate::features::watch::schema::GitDirChange;
use crate::input::{BranchRef, FullSha, NonEmpty, Paths, RepositoryId, Sha};
use crate::test_support::{Fixture, git, valid};

/// Procedures only take UUIDv7 repository ids.
const HISTORY: &str = "01920000-0000-7000-8000-000000000001";
const LOCKED: &str = "01920000-0000-7000-8000-000000000002";
const MISSING: &str = "01920000-0000-7000-8000-000000000003";
const AMENDED: &str = "01920000-0000-7000-8000-000000000004";

/// The commit SHA no repository has.
const UNKNOWN_SHA: &str = "0123456789abcdef0123456789abcdef01234567";

fn id(value: &str) -> RepositoryId {
    RepositoryId::parse(value).unwrap()
}

/// `error`'s code, as the renderer knew it, and its message.
fn api_error(error: ApiError) -> (&'static str, String) {
    (error.code().code(), error.message().to_owned())
}

/// The API over `fx`'s repositories, with the installed git.
fn api(fx: &Fixture) -> GitApi {
    GitApi::with(fx.repos.git_repos(), GitVersion::new())
}

/// The repository with some history (see `create_history_repo`), as `HISTORY`.
async fn history(fx: &Fixture) {
    let repo = fx.create_history_repo("history").await;
    fx.add_path(HISTORY, repo.path());
}

#[test]
fn reports_git_errors_as_api_errors_the_renderer_can_show() {
    let fx = Fixture::new();
    let locked = fx.create_repo("locked");
    fs::write(locked.join("file.txt"), "x").unwrap();
    fs::write(locked.join(".git").join("index.lock"), "").unwrap();
    fx.add_path(LOCKED, &locked);
    let api = api(&fx);
    smol::block_on(async {
        history(&fx).await;
        let sha = FullSha::parse(UNKNOWN_SHA).unwrap();

        let status = api.call("status.get", &id(MISSING), |repo| async move {
            get_status_since(&repo, None).await
        });
        assert_eq!(
            api_error(status.await.unwrap_err()),
            ("NOT_FOUND", "Repository not found.".to_owned())
        );

        let paths = Paths::parse(["file.txt"]).unwrap();
        let staged = api.call("staging.stage", &id(LOCKED), |repo| async move {
            stage(&repo, &paths).await
        });
        assert_eq!(
            api_error(staged.await.unwrap_err()),
            (
                "CONFLICT",
                "Another git process is running in this repository.".to_owned()
            )
        );

        let commit = Sha::from(sha.clone());
        let files = api.call("diff.commitFiles", &id(HISTORY), |repo| async move {
            get_commit_files(&repo, &commit).await
        });
        let (code, message) = api_error(files.await.unwrap_err());
        assert_eq!(code, "INTERNAL_SERVER_ERROR");
        assert!(message.contains("bad object"), "{message}");

        let pulled = api.call("remote.pull", &id(HISTORY), |repo| async move {
            pull(&repo, None).await
        });
        assert_eq!(
            api_error(pulled.await.unwrap_err()),
            (
                "PRECONDITION_FAILED",
                "main doesn't track a remote branch.".to_owned()
            )
        );

        let pushed = api.call("remote.push", &id(HISTORY), |repo| async move {
            push(&repo, false).await
        });
        assert_eq!(
            api_error(pushed.await.unwrap_err()),
            (
                "PRECONDITION_FAILED",
                "There's no remote to push to.".to_owned()
            )
        );

        let popped = api.call("stash.pop", &id(HISTORY), |repo| async move {
            pop_stash(&repo, &sha).await
        });
        assert_eq!(
            api_error(popped.await.unwrap_err()),
            (
                "CONFLICT",
                "The stash is gone: the stashes changed before it could be popped.".to_owned()
            )
        );
    });
}

#[test]
fn runs_nothing_with_a_git_gitto_doesnt_support() {
    let fx = Fixture::new();
    let output = Arc::new(Mutex::new(Some(
        "git version 2.39.3 (Apple Git-146)\n".to_owned(),
    )));
    let read = Arc::clone(&output);
    let version = GitVersion::with_reader(move || {
        let output = read.lock().unwrap().clone();
        async move { output }
    });
    let outdated = GitApi::with(fx.repos.git_repos(), version);
    smol::block_on(async {
        history(&fx).await;
        let ran = Arc::new(AtomicBool::new(false));
        let status = || {
            let ran = Arc::clone(&ran);
            outdated.call("status.get", &id(HISTORY), |repo| async move {
                ran.store(true, Ordering::SeqCst);
                get_status_since(&repo, None).await
            })
        };

        let install = check(outdated.version()).await;
        assert_eq!(install.version.as_deref(), Some("2.39.3"));
        assert_eq!(install.required, "2.41");
        assert!(!install.supported);
        assert_eq!(
            api_error(status().await.unwrap_err()),
            (
                "PRECONDITION_FAILED",
                "Gitto needs Git 2.41 or newer, but Git 2.39.3 is installed.".to_owned()
            )
        );

        *output.lock().unwrap() = None;
        assert_eq!(
            api_error(status().await.unwrap_err()),
            (
                "PRECONDITION_FAILED",
                "Gitto couldn't find Git. Install Git 2.41 or newer.".to_owned()
            )
        );
        assert!(!ran.load(Ordering::SeqCst));

        // Updated since: it works again without a restart.
        *output.lock().unwrap() = Some("git version 2.51.0\n".to_owned());
        let StatusReply::Status(status) = status().await.unwrap() else {
            panic!("expected a full status");
        };
        assert!(
            matches!(&status.head, Head::Branch { name, .. } if name == "main"),
            "{:?}",
            status.head
        );
    });
}

#[test]
fn merges_a_branch_saying_what_it_did_or_why_it_didnt() {
    let fx = Fixture::new();
    let api = api(&fx);
    smol::block_on(async {
        history(&fx).await;
        // The UI's input, checked as it's parsed, then the procedure.
        let merge = |reference: &str, into: &str| {
            let input = BranchRef::parse(reference).and_then(|reference| {
                let into = NonEmpty::parse(into)?;
                Ok((reference, into))
            });
            let api = api.clone();
            async move {
                let (reference, into) = input.map_err(|error| api.fail("branch.merge", error))?;
                api.call("branch.merge", &id(HISTORY), |repo| async move {
                    merge_branch(&repo, &reference, &into).await
                })
                .await
            }
        };

        // `side` is merged into `main` already.
        assert_eq!(
            merge("refs/heads/side", "main").await.unwrap(),
            MergeOutcome::UpToDate
        );
        assert_eq!(
            api_error(merge("refs/heads/main", "main").await.unwrap_err()),
            (
                "PRECONDITION_FAILED",
                "Can't merge main into itself.".to_owned()
            )
        );
        assert_eq!(
            api_error(merge("refs/heads/side", "other").await.unwrap_err()),
            (
                "CONFLICT",
                "Switched from other to main meanwhile, so nothing was merged.".to_owned()
            )
        );
        let (code, _) = api_error(merge("refs/tags/v1", "main").await.unwrap_err());
        assert_eq!(code, "BAD_REQUEST");
        let (code, _) = api_error(merge("refs/heads/side", "").await.unwrap_err());
        assert_eq!(code, "BAD_REQUEST");
        // Past the input's check, but not a branch: a link to the remote's default one.
        assert_eq!(
            api_error(merge("refs/remotes/origin/HEAD", "main").await.unwrap_err()),
            (
                "PRECONDITION_FAILED",
                "refs/remotes/origin/HEAD isn't a branch.".to_owned()
            )
        );
    });
}

#[test]
fn skips_a_status_the_caller_already_has() {
    let fx = Fixture::new();
    let api = api(&fx);
    smol::block_on(async {
        history(&fx).await;
        let status = |since: Option<String>| {
            api.call("status.get", &id(HISTORY), |repo| async move {
                get_status_since(&repo, since.as_deref()).await
            })
        };
        let StatusReply::Status(full) = status(None).await.unwrap() else {
            panic!("expected a full status");
        };

        assert!(matches!(
            status(Some(full.version.clone())).await.unwrap(),
            StatusReply::Unchanged { .. }
        ));
        assert_eq!(
            status(Some("an older version".to_owned())).await.unwrap(),
            StatusReply::Status(full)
        );
    });
}

#[test]
fn tells_the_renderer_a_push_it_can_force_apart_from_other_failures() {
    let fx = Fixture::new();
    let remote = fx.create_repo("amended-remote");
    git(&remote, ["commit", "-q", "--allow-empty", "-m", "first"]);
    let path = fx.clone_repo("amended", &remote);
    git(
        &path,
        ["commit", "-q", "--amend", "--allow-empty", "-m", "reworded"],
    );
    fx.add_path(AMENDED, &path);
    let api = api(&fx);
    smol::block_on(async {
        let error = api
            .call("remote.push", &id(AMENDED), |repo| async move {
                push(&repo, false).await
            })
            .await
            .unwrap_err();
        assert_eq!(error.code(), ApiCode::PushRewritten);
        assert_eq!(error.status(), 409);
        assert_eq!(
            api_error(error),
            (
                "PUSH_REWRITTEN",
                "origin/main still has main's commits from before a rebase or amend.".to_owned()
            )
        );
    });
}

// Beyond the router's tests: what `withRepo`, the handlers and `server.ts` did that they didn't
// test.

#[test]
fn takes_only_uuid_v7_repository_ids() {
    let error = RepositoryId::parse("history").unwrap_err();
    assert_eq!(ApiError::from(error).code().code(), "BAD_REQUEST");
}

#[test]
fn takes_only_the_values_the_contracts_enums_have() {
    use crate::features::conflicts::schema::Side;
    use crate::features::operation::schema::OperationKind;
    use crate::features::staging::discard::DiscardSide;
    use crate::schema::FileStatus;

    fn parses<T: serde::de::DeserializeOwned>(value: &str) -> bool {
        serde_json::from_str::<T>(&format!("{value:?}")).is_ok()
    }

    for kind in ["merge", "rebase", "cherry-pick", "revert", "am"] {
        assert!(parses::<OperationKind>(kind), "{kind}");
    }
    assert!(!parses::<OperationKind>("cherrypick"));
    assert!(!parses::<OperationKind>("Merge"));
    assert!(parses::<Side>("ours") && parses::<Side>("theirs"));
    assert!(!parses::<Side>("base"));
    assert!(parses::<DiscardSide>("staged") && parses::<DiscardSide>("unstaged"));
    assert!(!parses::<DiscardSide>("both"));
    for status in [
        "modified",
        "added",
        "deleted",
        "renamed",
        "copied",
        "typechange",
        "untracked",
        "conflicted",
    ] {
        assert!(parses::<FileStatus>(status), "{status}");
    }
    assert!(!parses::<FileStatus>("ignored"));
}

#[test]
fn reports_bugs_with_the_procedure_they_failed_and_only_bugs() {
    let fx = Fixture::new();
    let reported = Arc::new(Mutex::new(Vec::new()));
    let report = Arc::clone(&reported);
    let api = api(&fx).on_bug(move |error, procedure| {
        report
            .lock()
            .unwrap()
            .push((error.code().code(), procedure.to_owned()));
    });
    smol::block_on(async {
        history(&fx).await;
        let commit = Sha::parse(UNKNOWN_SHA).unwrap();
        let files = api.call("diff.commitFiles", &id(HISTORY), |repo| async move {
            get_commit_files(&repo, &commit).await
        });
        assert!(files.await.unwrap_err().is_bug());
        let missing = api.call("status.get", &id(MISSING), |repo| async move {
            get_status_since(&repo, None).await
        });
        assert!(!missing.await.unwrap_err().is_bug());
        let bug = api.call("history.log", &id(HISTORY), |_| async {
            Err::<(), _>(Error::Bug("Unexpected output.".to_owned()))
        });
        assert_eq!(
            api_error(bug.await.unwrap_err()),
            ("INTERNAL_SERVER_ERROR", "Internal server error".to_owned())
        );
        let input = api.fail("branch.merge", BranchRef::parse("v1").unwrap_err());
        assert_eq!(input.message(), "Input validation failed");
    });
    assert_eq!(
        *reported.lock().unwrap(),
        [
            ("INTERNAL_SERVER_ERROR", "diff.commitFiles".to_owned()),
            ("INTERNAL_SERVER_ERROR", "history.log".to_owned()),
            ("BAD_REQUEST", "branch.merge".to_owned()),
        ]
    );
}

/// A list of repositories that can't be read, as its database is locked; counts the lookups.
#[derive(Default)]
struct Locked(Arc<AtomicUsize>);

impl RepositoryLookup for Locked {
    fn repository_path<'a>(
        &'a self,
        _: &'a RepositoryId,
    ) -> BoxFuture<'a, Result<Option<PathBuf>, RepositoryListError>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        let locked = std::io::Error::other("database is locked");
        Box::pin(std::future::ready(Err(RepositoryListError::new(
            ListAction::Read,
            &locked,
        ))))
    }
}

#[test]
fn tells_the_ui_why_the_list_of_repositories_couldnt_be_read() {
    let reported = Arc::new(AtomicUsize::new(0));
    let report = Arc::clone(&reported);
    let api = GitApi::new(Locked::default()).on_bug(move |_, _| {
        report.fetch_add(1, Ordering::SeqCst);
    });
    let error = smol::block_on(api.call("status.get", &id(HISTORY), |repo| async move {
        get_status_since(&repo, None).await
    }))
    .unwrap_err();
    assert_eq!(
        api_error(error.clone()),
        (
            "INTERNAL_SERVER_ERROR",
            "Gitto couldn't read its list of repositories: database is locked.".to_owned()
        )
    );
    assert!(error.is_bug());
    assert!(matches!(error.error(), Error::RepositoryList(_)));
    assert_eq!(reported.load(Ordering::SeqCst), 1);
}

#[test]
fn checks_the_installed_git_before_looking_the_repository_up() {
    let lookups = Arc::new(AtomicUsize::new(0));
    let version = GitVersion::with_reader(|| async { None });
    let api = GitApi::with(
        crate::core::repo::GitRepos::new(Locked(Arc::clone(&lookups))),
        version,
    );
    let error = smol::block_on(api.open(&id(HISTORY))).err().unwrap();
    assert!(matches!(error, Error::UnsupportedGit { .. }), "{error:?}");
    assert_eq!(lookups.load(Ordering::SeqCst), 0);
}

#[test]
fn pulls_the_checked_out_branch_or_the_one_named() {
    let fx = Fixture::new();
    let (upstream, path) = fx.create_clone("clone");
    git(&path, ["branch", "-q", "--track", "other", "origin/main"]);
    git(&upstream, ["commit", "-q", "--allow-empty", "-m", "second"]);
    smol::block_on(async {
        let repo = fx.repos.open("clone").await.unwrap();
        let other = NonEmpty::parse("other").unwrap();
        pull(&repo, Some(&other)).await.unwrap();
        let upstream_head = git(&upstream, ["rev-parse", "HEAD"]);
        assert_eq!(git(&path, ["rev-parse", "other"]), upstream_head);
        assert_ne!(git(&path, ["rev-parse", "main"]), upstream_head);

        pull(&repo, None).await.unwrap();
        assert_eq!(git(&path, ["rev-parse", "main"]), upstream_head);
    });
}

#[test]
fn watches_until_its_dropped_and_says_why_it_cant() {
    let fx = Fixture::new();
    let path = fx.create_repo("watched");
    git(&path, ["commit", "-q", "--allow-empty", "-m", "first"]);
    let watched = "01920000-0000-7000-8000-000000000005";
    fx.add_path(watched, &path);
    let api = api(&fx);
    smol::block_on(async {
        let mut missing = api.watch_git_dir(&id(MISSING));
        assert_eq!(
            api_error(missing.next().await.unwrap().unwrap_err()),
            ("NOT_FOUND", "Repository not found.".to_owned())
        );
        assert!(missing.next().await.is_none());

        let mut tree = api.watch_working_tree(&id(watched));
        let (change, ()) = futures::join!(tree.next(), async {
            Timer::after(Duration::from_millis(200)).await;
            fs::write(path.join("file.txt"), "x").unwrap();
        });
        assert!(matches!(change, Some(Ok(()))), "{change:?}");
        drop(tree);

        let mut git_dir = api.watch_git_dir(&id(watched));
        let (change, ()) = futures::join!(git_dir.next(), async {
            Timer::after(Duration::from_millis(200)).await;
            git(&path, ["commit", "-q", "--allow-empty", "-m", "second"]);
        });
        let change = change.unwrap().unwrap();
        assert!(change.contains(&GitDirChange::Refs), "{change:?}");
    });
}

#[test]
fn calls_can_run_on_another_thread() {
    fn send<T: Send>(_: &T) {}

    let fx = Fixture::new();
    let api = api(&fx);
    let reference = valid::<BranchRef>("refs/heads/side");
    let into = valid::<NonEmpty>("main");
    let merge = api.call("branch.merge", &id(HISTORY), |repo| async move {
        merge_branch(&repo, &reference, &into).await
    });
    send(&merge);
    send(&api.open(&id(HISTORY)));
    send(&api.watch_git_dir(&id(HISTORY)));
}

#[test]
fn logs_a_failure_with_the_command_git_ran() {
    let error = crate::core::errors::GitError::new(
        "fatal: bad object 0123",
        vec!["diff-tree".into(), "0123".into()],
        Some(128),
        "fatal: bad object 0123\n",
    );
    assert_eq!(
        super::log_line("diff.commitFiles", &ApiError::from(Error::Git(error))),
        "[git] diff.commitFiles failed (INTERNAL_SERVER_ERROR): fatal: bad object 0123\n  \
         git diff-tree 0123 (exit code 128)"
    );
    assert_eq!(
        super::log_line("commit.create", &ApiError::from(Error::HeadMoved)),
        "[git] commit.create failed (CONFLICT): The last commit changed before it could be \
         amended, so nothing was amended."
    );
}
