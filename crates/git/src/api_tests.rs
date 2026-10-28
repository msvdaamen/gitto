//! What the UI gets from the features, errors and all. Ported from `packages/git/src/router.test.ts`:
//! the Electron app called the features through an oRPC router, which checked the input and the
//! installed git, and turned errors into codes the renderer showed. The UI calls the features
//! directly here, so these call them as the router's procedures did, and check what
//! [`Error::api`] says of their errors.

use std::fs;

use crate::core::errors::{ApiCode, Error};
use crate::core::version::GitVersion;
use crate::features::branch::merge::merge_branch;
use crate::features::branch::schema::MergeOutcome;
use crate::features::diff::commands::get_commit_files;
use crate::features::remote::commands::pull;
use crate::features::remote::push::push;
use crate::features::staging::commands::stage;
use crate::features::stash::commands::pop_stash;
use crate::features::status::commands::{get_status, get_status_since};
use crate::features::status::schema::{Head, StatusReply};
use crate::input::{BranchRef, Sha};
use crate::test_support::{Fixture, git};

/// `error`'s code, as the renderer knew it, and its message.
fn api_error(error: Error) -> (&'static str, String) {
    (error.api().code(), error.to_string())
}

/// The commit SHA no repository has.
const UNKNOWN_SHA: &str = "0123456789abcdef0123456789abcdef01234567";

#[test]
fn reports_git_errors_as_api_errors_the_renderer_can_show() {
    let fx = Fixture::new();
    let locked = fx.create_repo("locked");
    fs::write(locked.join("file.txt"), "x").unwrap();
    fs::write(locked.join(".git").join("index.lock"), "").unwrap();
    smol::block_on(async {
        let repo = fx.create_history_repo("history").await;

        let Err(error) = fx.repos.open("missing").await else {
            panic!("opened a repository that isn't there");
        };
        assert_eq!(
            api_error(error),
            ("NOT_FOUND", "Repository not found.".to_owned())
        );

        let locked = fx.repos.open("locked").await.unwrap();
        let error = stage(&locked, &["file.txt".to_owned()]).await.unwrap_err();
        assert_eq!(
            api_error(error),
            (
                "CONFLICT",
                "Another git process is running in this repository.".to_owned()
            )
        );

        let sha = Sha::parse(UNKNOWN_SHA).unwrap();
        let (code, message) = api_error(get_commit_files(&repo, &sha).await.unwrap_err());
        assert_eq!(code, "INTERNAL_SERVER_ERROR");
        assert!(message.contains("bad object"), "{message}");

        assert_eq!(
            api_error(pull(&repo).await.unwrap_err()),
            (
                "PRECONDITION_FAILED",
                "main doesn't track a remote branch.".to_owned()
            )
        );

        assert_eq!(
            api_error(push(&repo, false).await.unwrap_err()),
            (
                "PRECONDITION_FAILED",
                "There's no remote to push to.".to_owned()
            )
        );

        assert_eq!(
            api_error(pop_stash(&repo, &sha).await.unwrap_err()),
            (
                "CONFLICT",
                "The stash is gone: the stashes changed before it could be popped.".to_owned()
            )
        );
    });
}

#[test]
fn runs_nothing_with_a_git_gitto_doesnt_support() {
    use std::sync::{Arc, Mutex};

    let fx = Fixture::new();
    let output = Arc::new(Mutex::new(Some(
        "git version 2.39.3 (Apple Git-146)\n".to_owned(),
    )));
    let read = Arc::clone(&output);
    let version = GitVersion::with_reader(move || {
        let output = read.lock().unwrap().clone();
        async move { output }
    });
    smol::block_on(async {
        let repo = fx.create_history_repo("history").await;
        // What the router did before each procedure on a repository.
        let status = || async {
            version.require().await?;
            get_status(&repo).await
        };

        let install = version.check().await;
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

        // Updated since: it works again without a restart.
        *output.lock().unwrap() = Some("git version 2.51.0\n".to_owned());
        let status = status().await.unwrap();
        assert!(
            matches!(&status.summary.head, Head::Branch { name, .. } if name == "main"),
            "{:?}",
            status.summary.head
        );
    });
}

#[test]
fn merges_a_branch_saying_what_it_did_or_why_it_didnt() {
    let fx = Fixture::new();
    smol::block_on(async {
        let repo = fx.create_history_repo("history").await;
        // The router's input check, then the procedure.
        let merge = |reference: &'static str, into: &'static str| {
            let repo = repo.clone();
            async move {
                let reference = BranchRef::parse(reference)?;
                merge_branch(&repo, &reference, into).await
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
        // Past the input's check, but not a branch: a link to the remote's default one. Refused
        // as input too.
        assert_eq!(
            api_error(merge("refs/remotes/origin/HEAD", "main").await.unwrap_err()),
            (
                "BAD_REQUEST",
                "refs/remotes/origin/HEAD isn't a branch.".to_owned()
            )
        );
    });
}

#[test]
fn skips_a_status_the_caller_already_has() {
    let fx = Fixture::new();
    smol::block_on(async {
        let repo = fx.create_history_repo("history").await;
        let StatusReply::Status(status) = get_status_since(&repo, None).await.unwrap() else {
            panic!("expected a full status");
        };

        assert!(matches!(
            get_status_since(&repo, Some(&status.version))
                .await
                .unwrap(),
            StatusReply::Unchanged { .. }
        ));
        assert_eq!(
            get_status_since(&repo, Some("an older version"))
                .await
                .unwrap(),
            StatusReply::Status(status)
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
    smol::block_on(async {
        let repo = fx.repos.open("amended").await.unwrap();
        let error = push(&repo, false).await.unwrap_err();
        assert_eq!(error.api(), ApiCode::PushRewritten);
        assert_eq!(
            api_error(error),
            (
                "PUSH_REWRITTEN",
                "origin/main still has main's commits from before a rebase or amend.".to_owned()
            )
        );
    });
}
