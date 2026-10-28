//! Reading what the home page shows of a repository. Ported from `overview/commands.ts`.

use std::time::UNIX_EPOCH;

use super::parse::{REFLOG_FORMAT, parse_reflog, pick_remote};
use super::schema::{Activity, HostedRemote, Overview};
use crate::core::errors::Result;
use crate::core::repo::Repo;
use crate::features::history::parse::millis;

/// How many of the latest entries of HEAD's reflog are read, of which some aren't shown.
const REFLOG_ENTRIES: usize = 50;

/// How many of them are sent: the home page only shows the latest few of all repositories.
const MAX_ACTIVITY: usize = 10;

/// What the home page shows of a repository, beyond its status: where it's hosted, when it was
/// last fetched and committed to, and what was last done in it. All of it read, none written.
pub async fn get_overview(repo: &Repo) -> Result<Overview> {
    let has_head = repo.has_head().await;
    let committed_at = async {
        if has_head {
            read_committed_at(repo).await.map(Some)
        } else {
            Ok(None)
        }
    };
    let activity = async {
        if has_head {
            read_activity(repo).await
        } else {
            Ok(Vec::new())
        }
    };
    let (remote, fetched_at, committed_at, activity) = futures::try_join!(
        read_remote(repo),
        read_fetched_at(repo),
        committed_at,
        activity
    )?;
    Ok(Overview {
        remote,
        fetched_at,
        committed_at,
        activity,
    })
}

/// The remote the home page names (see `pick_remote`); `None` when there are none.
async fn read_remote(repo: &Repo) -> Result<Option<HostedRemote>> {
    match repo
        .read(["config", "-z", "--get-regexp", r"^remote\..+\.url$"])
        .await
    {
        Ok(output) => Ok(pick_remote(&output)),
        // There are no remotes.
        Err(error) if error.is_exit(1) => Ok(None),
        Err(error) => Err(error),
    }
}

/// When FETCH_HEAD, which every fetch writes, was last written, in milliseconds since the epoch;
/// `None` if it never was.
async fn read_fetched_at(repo: &Repo) -> Result<Option<i64>> {
    // In the worktree's own git directory.
    let path = repo
        .read([
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            "FETCH_HEAD",
        ])
        .await?;
    let Ok(modified) = async_fs::metadata(path.trim())
        .await
        .and_then(|metadata| metadata.modified())
    else {
        return Ok(None);
    };
    // Before the epoch for a time before it, as Node's `mtimeMs`.
    let in_millis =
        |duration: std::time::Duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX);
    Ok(Some(match modified.duration_since(UNIX_EPOCH) {
        Ok(since) => in_millis(since),
        Err(before) => -in_millis(before.duration()),
    }))
}

/// When HEAD's commit was made, in milliseconds since the epoch.
async fn read_committed_at(repo: &Repo) -> Result<i64> {
    let output = repo.read(["log", "-1", "--format=%ct", "HEAD"]).await?;
    millis(&output)
}

/// HEAD's latest activity, from its reflog: at most `MAX_ACTIVITY` entries.
async fn read_activity(repo: &Repo) -> Result<Vec<Activity>> {
    let output = repo
        .read([
            "log",
            "-g",
            "-z",
            "--date=unix",
            REFLOG_FORMAT,
            &format!("--max-count={REFLOG_ENTRIES}"),
            "HEAD",
        ])
        .await?;
    let mut activity = parse_reflog(&output)?;
    activity.truncate(MAX_ACTIVITY);
    Ok(activity)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::SystemTime;

    use super::*;
    use crate::features::overview::schema::ActivityKind;
    use crate::test_support::{Fixture, git};

    #[test]
    fn has_nothing_to_tell_of_a_new_repository() {
        let fx = Fixture::new();
        fx.create_repo("empty");
        smol::block_on(async {
            let repo = fx.repos.open("empty").await.unwrap();
            assert_eq!(
                get_overview(&repo).await.unwrap(),
                Overview {
                    remote: None,
                    fetched_at: None,
                    committed_at: None,
                    activity: vec![],
                }
            );
        });
    }

    #[test]
    fn tells_where_a_repository_is_hosted_when_it_was_fetched_and_committed_to_and_what_was_done() {
        let fx = Fixture::new();
        let upstream = fx.create_repo("upstream");
        git(&upstream, ["commit", "--allow-empty", "-qm", "first"]);
        let path = fx.clone_repo("clone", &upstream);
        git(&path, ["checkout", "-qb", "feature"]);
        fs::write(path.join("a.txt"), "a\n").unwrap();
        git(&path, ["add", "a.txt"]);
        git(&path, ["commit", "-qm", "Add a"]);
        git(&path, ["fetch", "-q"]);

        smol::block_on(async {
            let overview = get_overview(&fx.repos.open("clone").await.unwrap())
                .await
                .unwrap();
            assert_eq!(
                overview.remote,
                Some(HostedRemote {
                    name: "origin".into(),
                    url: upstream.display().to_string(),
                })
            );
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;
            let fetched_at = overview.fetched_at.unwrap();
            assert!(
                (now - 60_000..=now).contains(&fetched_at),
                "{fetched_at} {now}"
            );
            let committed: i64 = git(&path, ["log", "-1", "--format=%ct"]).parse().unwrap();
            assert_eq!(overview.committed_at, Some(committed * 1000));
            let activity: Vec<_> = overview
                .activity
                .iter()
                .map(|entry| (entry.kind, entry.target.as_deref(), entry.subject.as_str()))
                .collect();
            assert_eq!(
                activity,
                [
                    (ActivityKind::Commit, None, "Add a"),
                    (ActivityKind::Checkout, Some("feature"), "first"),
                    (ActivityKind::Clone, None, "first"),
                ]
            );
        });
    }
}
