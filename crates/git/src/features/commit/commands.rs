//! Committing, amending and rewording. Ported from `commit/commands.ts`, with the input check of
//! `commit/contract.ts` ([`Message`]).

use std::fmt;
use std::future::Future;
use std::ops::Deref;

use serde::{Deserialize, Serialize};

use crate::core::errors::{Error, Result};
use crate::core::repo::{GitCommand, Repo, current_branch, resolve_ref};
use crate::core::runner::RunOptions;
use crate::input::{FullSha, Sha};

/// A commit's message: anything but only whitespace. Not trimmed: an amend, or a reword, commits it
/// exactly as it's sent, ending in a newline or not.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Message(String);

impl Message {
    /// `value`, if it has something but whitespace; `Error::InvalidInput` otherwise.
    pub fn parse(value: impl Into<String>) -> Result<Message> {
        let value = value.into();
        // JS's `\S`: `\s` also has the byte order mark, which Unicode's whitespace doesn't.
        if value.chars().any(|c| !c.is_whitespace() && c != '\u{feff}') {
            Ok(Message(value))
        } else {
            Err(Error::InvalidInput("The message is empty.".to_owned()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for Message {
    type Error = Error;

    fn try_from(value: String) -> Result<Message> {
        Message::parse(value)
    }
}

impl From<Message> for String {
    fn from(message: Message) -> String {
        message.0
    }
}

impl Deref for Message {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Commits what's staged, with `message` as it's written. With `amend`, HEAD's SHA, replaces that
/// commit with it instead, with what's staged added to it, as long as HEAD is still that commit:
/// fails with `Error::HeadMoved` if it has moved on since, so another commit is never amended by
/// mistake. Nothing needs to be staged to only reword it: an amend may leave the commit empty, so
/// an empty one can be reworded.
///
/// Its place in the repository's writes is taken when it's first polled (see `Repo::exclusive`).
pub fn create_commit(
    repo: &Repo,
    message: &Message,
    amend: Option<&FullSha>,
) -> impl Future<Output = Result<()>> + Send + 'static + use<> {
    let message = message.as_str().to_owned();
    let amend = amend.map(|sha| sha.as_str().to_owned());
    repo.exclusive(move |run| async move {
        if let Some(amend) = amend {
            return amend_head(&run, &amend, &message, &[]).await;
        }
        // A new commit's message is cleaned up by git, but for the spaces it'd leave before the
        // subject.
        run.run_with(
            ["commit", "-F", "-"],
            RunOptions::default().stdin(message.trim()),
        )
        .await?;
        Ok(())
    })
}

/// Rewrites the message of HEAD's commit, `sha`, to `message` as it's written, as long as HEAD is
/// still that commit; what's staged is left out of it, and stays staged. Resolves to the rewritten
/// commit's full SHA. The commit's hooks run as for `git commit --amend`, pre-commit too: as when
/// git rewords a commit in a rebase.
///
/// Its own function, rather than an option of `create_commit`, for that SHA, which committing
/// doesn't look up. Its place in the repository's writes is taken when it's first polled.
pub fn reword_commit(
    repo: &Repo,
    sha: &FullSha,
    message: &Message,
) -> impl Future<Output = Result<String>> + Send + 'static + use<> {
    let sha = sha.as_str().to_owned();
    let message = message.as_str().to_owned();
    // One write, so no other commit of Gitto's can land before the rewritten one's SHA is read.
    repo.exclusive(move |run| async move {
        // With no paths, `--only` commits none of what's staged.
        amend_head(&run, &sha, &message, &["--only"]).await?;
        // A commit was just made, so HEAD has one.
        resolve_ref(&run, "HEAD")
            .await?
            .ok_or_else(|| Error::Bug("HEAD has no commit after a commit.".to_owned()))
    })
}

/// Replaces HEAD's commit, `sha`, with one of `message`, and what's staged unless `args` say
/// otherwise; fails if HEAD has moved on. Run in one write, so no other commit of Gitto's can land
/// between checking HEAD and amending it.
async fn amend_head(run: &GitCommand, sha: &str, message: &str, args: &[&str]) -> Result<()> {
    if resolve_ref(run, "HEAD").await?.as_deref() != Some(sha) {
        return Err(Error::HeadMoved);
    }
    // An amended message is kept exactly as sent, whatever `commit.cleanup` says, as what wasn't
    // edited of it is as it was written.
    let mut command = vec!["commit", "--amend", "--allow-empty", "--cleanup=verbatim"];
    command.extend_from_slice(args);
    command.extend(["-F", "-"]);
    run.run_with(command, RunOptions::default().stdin(message))
        .await?;
    Ok(())
}

/// A commit's message exactly as it was written, e.g. the last one's to amend it; the log's
/// subject joins its first lines.
pub async fn get_commit_message(repo: &Repo, sha: &Sha) -> Result<String> {
    let mut output = repo
        .read([
            "log",
            "--max-count=1",
            "--format=%B",
            "--end-of-options",
            sha.as_str(),
            "--",
        ])
        .await?;
    // The log ends each entry with a newline of its own.
    if output.ends_with('\n') {
        output.pop();
    }
    Ok(output)
}

/// The remote branch `git push` would update that already has the commit `sha`, e.g.
/// `origin/feature`; `None` if amending it needs no force-push. Not a branch the checked-out one
/// was only created from, e.g. `origin/main`, which amending doesn't rewrite; and none with HEAD
/// detached, as amending then rewrites no branch.
pub async fn get_pushed_to(repo: &Repo, sha: &Sha) -> Result<Option<String>> {
    let run = repo.git();
    // Where the repository's config pushes the branch. It fails when the config can't say, as
    // `git push` would; a broken repository fails the lookups around it too.
    let push = repo.read(["rev-parse", "--symbolic-full-name", "@{push}"]);
    let push = async move {
        Ok(match push.await {
            Ok(push) => push.trim().to_owned(),
            Err(_) => String::new(),
        })
    };
    let (branch, push) = futures::try_join!(current_branch(&run), push)?;
    // HEAD is detached: amending rewrites no branch.
    let Some(branch) = branch else {
        return Ok(None);
    };
    // Pushing to a branch of this repository (its remote is `.`) rewrites nothing published.
    if !push.is_empty() && !push.starts_with("refs/remotes/") {
        return Ok(None);
    }
    // When the config can't say, e.g. because the upstream has another name, a guess: the branch
    // of the same name on any remote, which `git push <remote>` updates.
    let pattern = if push.is_empty() {
        format!("refs/remotes/*/{branch}")
    } else {
        push
    };
    let pushed_to = repo
        .read([
            "for-each-ref",
            "--count=1",
            &format!("--contains={sha}"),
            "--format=%(refname:lstrip=2)",
            &pattern,
        ])
        .await?;
    let pushed_to = pushed_to.trim();
    Ok((!pushed_to.is_empty()).then(|| pushed_to.to_owned()))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::core::errors::GitErrorKind;
    use crate::features::history::commands::{PAGE, get_log};
    use crate::features::history::schema::{CommitRef, CommitRefKind};
    use crate::test_support::{Fixture, git};

    fn message(text: &str) -> Message {
        Message::parse(text).unwrap()
    }

    fn head(path: &Path) -> FullSha {
        FullSha::parse(git(path, ["rev-parse", "HEAD"])).unwrap()
    }

    /// The commit `repo`'s HEAD is at, as the UI would name it.
    fn at_head(repo: &Repo) -> Sha {
        Sha::parse(git(repo.path(), ["rev-parse", "HEAD"])).unwrap()
    }

    /// The repository the TS tests share, as their first one leaves it: "Initial commit", which
    /// adds `x y.txt`, on `main`.
    fn committed(fx: &Fixture) -> PathBuf {
        let path = fx.create_repo("empty");
        fs::write(path.join("x y.txt"), "hi\n").unwrap();
        git(&path, ["add", "x y.txt"]);
        git(&path, ["commit", "-q", "-m", "Initial commit\n\nWith body"]);
        path
    }

    async fn subjects(repo: &Repo) -> Vec<String> {
        let log = get_log(repo, PAGE).await.unwrap();
        log.into_iter().map(|commit| commit.subject).collect()
    }

    #[test]
    fn commits_whats_staged_with_the_messages_body() {
        let fx = Fixture::new();
        let path = fx.create_repo("empty");
        smol::block_on(async {
            let repo = fx.repos.open("empty").await.unwrap();
            fs::write(path.join("x y.txt"), "hi\n").unwrap();
            git(&path, ["add", "x y.txt"]);
            create_commit(&repo, &message("Initial commit\n\nWith body"), None)
                .await
                .unwrap();
            let log = get_log(&repo, PAGE).await.unwrap();
            assert_eq!(log.len(), 1);
            assert_eq!(log[0].subject, "Initial commit");
            assert_eq!(log[0].body, "With body");
            assert_eq!(
                log[0].refs,
                [CommitRef {
                    kind: CommitRefKind::Local,
                    name: "main".into(),
                    full_name: "refs/heads/main".into(),
                    current: true,
                }]
            );
        });
    }

    #[test]
    fn amends_the_last_commit_with_whats_staged_and_a_new_message() {
        let fx = Fixture::new();
        let path = committed(&fx);
        smol::block_on(async {
            let repo = fx.repos.open("empty").await.unwrap();
            fs::write(path.join("more.txt"), "more\n").unwrap();
            git(&path, ["add", "more.txt"]);
            create_commit(&repo, &message("Amended commit"), Some(&head(&path)))
                .await
                .unwrap();
            let log = get_log(&repo, PAGE).await.unwrap();
            assert_eq!(log.len(), 1);
            assert_eq!(log[0].subject, "Amended commit");
            assert_eq!(log[0].body, "");
            assert!(log[0].parents.is_empty());
            assert_eq!(
                git(&path, ["show", "--name-only", "--format=", "HEAD"])
                    .split('\n')
                    .collect::<Vec<_>>(),
                ["more.txt", "x y.txt"]
            );
        });
    }

    #[test]
    fn rewords_the_last_commit_with_nothing_staged() {
        let fx = Fixture::new();
        let path = committed(&fx);
        smol::block_on(async {
            let repo = fx.repos.open("empty").await.unwrap();
            create_commit(&repo, &message("Reworded\n\nNew body"), Some(&head(&path)))
                .await
                .unwrap();
            let log = get_log(&repo, PAGE).await.unwrap();
            assert_eq!(log.len(), 1);
            assert_eq!(log[0].subject, "Reworded");
            assert_eq!(log[0].body, "New body");
            assert!(log[0].parents.is_empty());
        });
    }

    #[test]
    fn rewords_an_empty_commit() {
        let fx = Fixture::new();
        let path = committed(&fx);
        smol::block_on(async {
            let repo = fx.repos.open("empty").await.unwrap();
            git(&path, ["commit", "-q", "--allow-empty", "-m", "Empty"]);
            create_commit(&repo, &message("Still empty"), Some(&head(&path)))
                .await
                .unwrap();
            assert_eq!(subjects(&repo).await[0], "Still empty");
        });
    }

    #[test]
    fn amends_before_a_commit_queued_while_it_checks_head_not_that_commit() {
        let fx = Fixture::new();
        let path = committed(&fx);
        smol::block_on(async {
            let repo = fx.repos.open("empty").await.unwrap();
            let amending = create_commit(&repo, &message("Amended first"), Some(&head(&path)));
            let other = repo.write(["commit", "-q", "--allow-empty", "-m", "Queued after"]);
            let (amended, committed) = futures::join!(amending, other);
            amended.unwrap();
            committed.unwrap();
            assert_eq!(
                subjects(&repo).await[..2],
                ["Queued after", "Amended first"]
            );
        });
    }

    #[test]
    fn amends_with_the_message_exactly_as_written_whatever_commit_cleanup_says() {
        let fx = Fixture::new();
        let path = committed(&fx);
        git(&path, ["config", "commit.cleanup", "strip"]);
        smol::block_on(async {
            let repo = fx.repos.open("empty").await.unwrap();
            let text = "Kept  \n\n\n#123 fixes it\nHard break  \n";
            create_commit(&repo, &message(text), Some(&head(&path)))
                .await
                .unwrap();
            assert_eq!(
                get_commit_message(&repo, &at_head(&repo)).await.unwrap(),
                text
            );
        });
    }

    #[test]
    fn records_the_message_in_utf8_whatever_i18n_commit_encoding_says() {
        let fx = Fixture::new();
        let path = committed(&fx);
        git(&path, ["config", "i18n.commitEncoding", "ISO-8859-1"]);
        smol::block_on(async {
            let repo = fx.repos.open("empty").await.unwrap();
            create_commit(&repo, &message("Café\n"), Some(&head(&path)))
                .await
                .unwrap();
            assert_eq!(
                get_commit_message(&repo, &at_head(&repo)).await.unwrap(),
                "Café\n"
            );
        });
    }

    #[test]
    fn trims_a_new_commits_message_and_amends_with_one_exactly_as_sent() {
        let fx = Fixture::new();
        let path = committed(&fx);
        smol::block_on(async {
            let repo = fx.repos.open("empty").await.unwrap();
            fs::write(path.join("trim.txt"), "trim\n").unwrap();
            git(&path, ["add", "trim.txt"]);
            create_commit(&repo, &message("  New  \n"), None)
                .await
                .unwrap();
            assert_eq!(
                get_commit_message(&repo, &at_head(&repo)).await.unwrap(),
                "New\n"
            );
            create_commit(&repo, &message("No newline"), Some(&head(&path)))
                .await
                .unwrap();
            assert_eq!(
                get_commit_message(&repo, &at_head(&repo)).await.unwrap(),
                "No newline"
            );
        });
    }

    #[test]
    fn explains_why_a_commit_failed() {
        let fx = Fixture::new();
        committed(&fx);
        smol::block_on(async {
            let repo = fx.repos.open("empty").await.unwrap();
            let error = create_commit(&repo, &message("nothing"), None)
                .await
                .unwrap_err();
            assert!(error.to_string().contains("nothing to commit"), "{error}");
        });
    }

    #[test]
    fn wont_amend_once_head_has_moved_on() {
        let fx = Fixture::new();
        let path = committed(&fx);
        smol::block_on(async {
            let repo = fx.repos.open("empty").await.unwrap();
            let amending = head(&path);
            git(
                &path,
                ["commit", "-q", "--allow-empty", "-m", "Made in a terminal"],
            );
            let error = create_commit(&repo, &message("Amend"), Some(&amending))
                .await
                .unwrap_err();
            assert!(matches!(error, Error::HeadMoved), "{error:?}");
            assert_eq!(subjects(&repo).await[0], "Made in a terminal");
        });
    }

    #[test]
    fn reports_why_it_couldnt_check_head() {
        let fx = Fixture::new();
        let gone = fx.create_repo("gone");
        smol::block_on(async {
            let repo = fx.repos.open("gone").await.unwrap();
            fs::remove_dir_all(gone.join(".git")).unwrap();
            let amend = FullSha::parse("a".repeat(40)).unwrap();
            let error = create_commit(&repo, &message("Amend"), Some(&amend))
                .await
                .unwrap_err();
            assert_eq!(
                error.as_git().map(|error| error.kind),
                Some(GitErrorKind::NotARepository),
                "{error:?}"
            );
        });
    }

    #[test]
    fn cant_amend_before_the_first_commit() {
        let fx = Fixture::new();
        let unborn = fx.create_repo("unborn");
        fs::write(unborn.join("a.txt"), "a\n").unwrap();
        git(&unborn, ["add", "a.txt"]);
        smol::block_on(async {
            let repo = fx.repos.open("unborn").await.unwrap();
            let amend = FullSha::parse("a".repeat(40)).unwrap();
            let error = create_commit(&repo, &message("Amend"), Some(&amend))
                .await
                .unwrap_err();
            assert!(matches!(error, Error::HeadMoved), "{error:?}");
        });
    }

    #[test]
    fn rewords_the_last_commit_only_leaving_whats_staged_out_of_it_and_staged() {
        let fx = Fixture::new();
        let path = fx.create_repo("reword");
        smol::block_on(async {
            let repo = fx.repos.open("reword").await.unwrap();
            fs::write(path.join("a.txt"), "a\n").unwrap();
            git(&path, ["add", "a.txt"]);
            git(&path, ["commit", "-q", "-m", "First"]);
            fs::write(path.join("staged.txt"), "staged\n").unwrap();
            git(&path, ["add", "staged.txt"]);

            let reworded = reword_commit(&repo, &head(&path), &message("Reworded\n"))
                .await
                .unwrap();
            assert_eq!(reworded, git(&path, ["rev-parse", "HEAD"]));
            assert_eq!(
                get_commit_message(&repo, &at_head(&repo)).await.unwrap(),
                "Reworded\n"
            );
            assert_eq!(
                git(&path, ["show", "--name-only", "--format=", "HEAD"]),
                "a.txt"
            );
            assert_eq!(
                git(&path, ["diff", "--cached", "--name-only"]),
                "staged.txt"
            );
        });
    }

    #[test]
    fn wont_reword_once_head_has_moved_on() {
        let fx = Fixture::new();
        let path = fx.create_repo("reword-moved");
        smol::block_on(async {
            let repo = fx.repos.open("reword-moved").await.unwrap();
            git(&path, ["commit", "-q", "--allow-empty", "-m", "First"]);
            let rewording = head(&path);
            git(
                &path,
                ["commit", "-q", "--allow-empty", "-m", "Made in a terminal"],
            );
            let error = reword_commit(&repo, &rewording, &message("Reworded"))
                .await
                .unwrap_err();
            assert!(matches!(error, Error::HeadMoved), "{error:?}");
            assert_eq!(
                get_commit_message(&repo, &at_head(&repo)).await.unwrap(),
                "Made in a terminal\n"
            );
        });
    }

    #[test]
    fn the_commit_message_is_as_written_first_lines_and_all_in_utf8() {
        let fx = Fixture::new();
        let path = fx.create_repo("message");
        let text = "First line\nsecond line\n\n#123 fixes it\n## Café";
        git(&path, ["commit", "-q", "--allow-empty", "-m", text]);
        git(&path, ["config", "i18n.logOutputEncoding", "ISO-8859-1"]);
        smol::block_on(async {
            let repo = fx.repos.open("message").await.unwrap();
            assert_eq!(
                get_commit_message(&repo, &at_head(&repo)).await.unwrap(),
                format!("{text}\n")
            );
        });
    }

    #[test]
    fn pushed_to_is_the_remote_branch_git_push_would_update_if_it_has_the_commit() {
        let fx = Fixture::new();
        let remote = fx.create_repo("remote");
        git(&remote, ["commit", "-q", "--allow-empty", "-m", "First"]);
        let path = fx.clone_repo("clone", &remote);
        smol::block_on(async {
            let repo = fx.repos.open("clone").await.unwrap();
            let pushed_to = || async {
                let head = at_head(&repo);
                get_pushed_to(&repo, &head).await
            };
            assert_eq!(pushed_to().await.unwrap().as_deref(), Some("origin/main"));

            // Only created from origin/main, which tracks it: amending rewrites `feature` alone.
            git(&path, ["switch", "-q", "-c", "feature", "origin/main"]);
            assert_eq!(pushed_to().await.unwrap(), None);

            git(&path, ["commit", "-q", "--allow-empty", "-m", "Feature"]);
            assert_eq!(pushed_to().await.unwrap(), None);
            // Pushed without setting it as the upstream.
            git(&path, ["push", "-q", "origin", "feature"]);
            assert_eq!(
                pushed_to().await.unwrap().as_deref(),
                Some("origin/feature")
            );

            // Pushed under another name: found through the push config.
            git(&path, ["switch", "-q", "-c", "mine"]);
            git(&path, ["push", "-q", "-u", "origin", "mine:theirs"]);
            assert_eq!(pushed_to().await.unwrap(), None);
            git(&path, ["config", "push.default", "upstream"]);
            assert_eq!(pushed_to().await.unwrap().as_deref(), Some("origin/theirs"));

            // A tag of the same name doesn't confuse which branch is checked out.
            git(&path, ["config", "--unset", "push.default"]);
            git(&path, ["switch", "-q", "feature"]);
            git(&path, ["tag", "feature"]);
            assert_eq!(
                pushed_to().await.unwrap().as_deref(),
                Some("origin/feature")
            );

            // Pushed to a branch of this repository.
            git(&path, ["config", "push.default", "upstream"]);
            git(&path, ["branch", "-q", "--set-upstream-to=main"]);
            git(&path, ["config", "branch.feature.remote", "."]);
            assert_eq!(pushed_to().await.unwrap(), None);

            git(&path, ["switch", "-q", "--detach"]);
            assert_eq!(pushed_to().await.unwrap(), None);
        });
    }

    #[test]
    fn messages_are_checked_as_the_contract_did() {
        assert_eq!(
            Message::parse(" \n\t").unwrap_err().to_string(),
            "The message is empty."
        );
        assert_eq!(Message::parse(" a\n").unwrap().as_str(), " a\n");
        assert!(serde_json::from_str::<Message>("\"\"").is_err());
    }
}
