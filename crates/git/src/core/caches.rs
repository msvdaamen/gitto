//! What's kept per repository around running git: its commit-graph, its index's refreshes, and
//! the status the UI was last shown. Ported from `packages/git/src/core/runner.ts`.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, UNIX_EPOCH};

use futures::future::{BoxFuture, FutureExt, Shared};

use super::errors::Result;
use super::git_dirs::path_from_git;
use super::queue::WriteQueue;
use super::runner::{RunOptions, lock, run_git, run_git_bytes, spawn_detached};
use super::trace::trace;

/// Writes each repository's commit-graph, once per run of the app. With it, git looks commits up
/// instead of parsing them, which makes sorting the history over all refs, when it needs that (see
/// `getLog`), several times faster: 0.5s instead of 3.4s on vscode. Fresh clones don't have one:
/// git only writes it in gc or maintenance.
///
/// Split, so it only adds the commits that aren't in it yet (in ~30ms when there are none), and
/// not through `WriteQueue`: it doesn't touch the index or refs, and can take seconds the first
/// time, which a commit shouldn't wait for. It's only a cache, so a failure (another git writing
/// it at the same time, a read-only repository) is left for next time.
#[derive(Default)]
pub(crate) struct CommitGraphs {
    writes: Mutex<HashMap<PathBuf, Shared<BoxFuture<'static, ()>>>>,
}

impl CommitGraphs {
    /// Writes `path`'s commit-graph if that hasn't been done yet; resolves once it's written. The
    /// write starts now, and runs to its end even if the future is dropped.
    pub fn update(&self, path: &Path) -> impl Future<Output = ()> + Send + 'static + use<> {
        lock(&self.writes)
            .entry(path.to_owned())
            .or_insert_with(|| {
                let write = run_git(
                    path,
                    [
                        "commit-graph",
                        "write",
                        "--reachable",
                        "--split",
                        "--no-progress",
                    ],
                    RunOptions::default(),
                );
                let path = path.to_owned();
                smol::spawn(async move {
                    match write.await {
                        Ok(_) => {
                            trace(format_args!("wrote the commit-graph of {}", path.display()))
                        }
                        Err(error) => trace(format_args!(
                            "couldn't write the commit-graph of {}: {error}",
                            path.display()
                        )),
                    }
                })
                .boxed()
                .shared()
            })
            .clone()
    }
}

/// How long after a write that rewrote files the index is refreshed: git compares the files' and
/// the index's timestamps in whole seconds.
pub(crate) const REFRESH_INDEX_AFTER: Duration = Duration::from_millis(1000);

/// A status that takes this long may be reading every file again (see `IndexRefreshes`).
pub(crate) const SLOW_STATUS: Duration = Duration::from_millis(500);

/// With this many files the index can't vouch for, a slow first status is put down to them. Fewer
/// are always there after a commit, say, and are read in no time: 0.1ms each in vscode.
pub(crate) const MANY_UNSURE_FILES: usize = 1000;

/// Refreshes each repository's index after a write that rewrote files in the working tree, like a
/// checkout. Git can't tell whether a file written in the same second as the index changed since,
/// so it reads it again, in full, whenever it compares the working tree with the index, until the
/// index is written in a later second. Gitto's reads never write it (see `ENV`), so every `status`
/// did: 0.5s instead of 80ms on vscode after switching to a branch 3,000 commits back.
///
/// Through `WriteQueue`, as it takes the index's lock, once a second has passed without another
/// such write. It's only a cache, so a failure (the user's git holding the lock, say) is left
/// alone.
///
/// A fresh clone is in that state too: 19,939 files written in the index's second in vscode, and
/// 1s rather than 25ms for every status. So the index is also refreshed when a repository's first
/// status of this run was slow and the index has many such files. Only then: it holds the lock for
/// as long as that status took, at a moment nobody asked Gitto to write, and a git command run in
/// a terminal meanwhile fails on it. So not for a status that was slow for another reason, like
/// files that aren't in the disk's cache yet, and a checkout made in a terminal later is left to
/// the next write there or here.
pub(crate) struct IndexRefreshes {
    writes: Arc<WriteQueue>,
    /// The refresh waiting to run for each repository, by the number it was scheduled as.
    timers: Mutex<HashMap<PathBuf, (u64, smol::Task<()>)>>,
    scheduled: AtomicU64,
    /// The repositories whose first status of this run has been timed.
    timed: Mutex<HashSet<PathBuf>>,
}

impl IndexRefreshes {
    pub fn new(writes: Arc<WriteQueue>) -> Self {
        IndexRefreshes {
            writes,
            timers: Mutex::new(HashMap::new()),
            scheduled: AtomicU64::new(0),
            timed: Mutex::new(HashSet::new()),
        }
    }

    /// Refreshes `path`'s index once `REFRESH_INDEX_AFTER` has passed without another call.
    pub fn schedule(self: &Arc<Self>, path: &Path) {
        let number = self.scheduled.fetch_add(1, Ordering::Relaxed);
        let this = Arc::clone(self);
        let key = path.to_owned();
        let mut timers = lock(&self.timers);
        let timer = smol::spawn(async move {
            async_io::Timer::after(REFRESH_INDEX_AFTER).await;
            {
                let mut timers = lock(&this.timers);
                if !timers
                    .get(&key)
                    .is_some_and(|(scheduled, _)| *scheduled == number)
                {
                    // Scheduled again since, which cancels this one as soon as it can.
                    return;
                }
                // Itself: left to run to its end, rather than cancelled by being dropped.
                if let Some((_, task)) = timers.remove(&key) {
                    task.detach();
                }
            }
            // `-q` and `--unmerged`: files that did change, and conflicts, aren't errors.
            let refresh = run_git(
                &key,
                ["update-index", "-q", "--unmerged", "--refresh"],
                RunOptions::default(),
            );
            match this.writes.run(key.clone(), refresh).await {
                Ok(_) => trace(format_args!("refreshed the index of {}", key.display())),
                Err(error) => trace(format_args!(
                    "couldn't refresh the index of {}: {error}",
                    key.display()
                )),
            }
        });
        // The one scheduled before is cancelled by being dropped, once the lock is let go.
        let previous = timers.insert(path.to_owned(), (number, timer));
        drop(timers);
        drop(previous);
    }

    /// Schedules a refresh if this status, which took `took`, is `path`'s first, was slow, and the
    /// index has many files it can't vouch for.
    pub fn status_took(self: &Arc<Self>, path: &Path, took: Duration) {
        if !lock(&self.timed).insert(path.to_owned()) {
            return;
        }
        if took < SLOW_STATUS {
            return;
        }
        let this = Arc::clone(self);
        let path = path.to_owned();
        spawn_detached(async move {
            // The repository is gone, say: nothing to refresh.
            let count = unsure_files(&path).await.unwrap_or(0);
            trace(format_args!(
                "the first status of {} took {}ms, with {count} unsure files",
                path.display(),
                took.as_millis()
            ));
            if count >= MANY_UNSURE_FILES {
                this.schedule(&path);
            }
        });
    }
}

/// How many of the index's files it can't vouch for: the ones it has as last written in the second
/// it was written itself, or later.
async fn unsure_files(path: &Path) -> Result<usize> {
    let (entries, index) = futures::try_join!(
        run_git(path, ["ls-files", "--debug"], RunOptions::default()),
        run_git_bytes(
            path,
            ["rev-parse", "--path-format=absolute", "--git-path", "index"],
            RunOptions::default(),
        ),
    )?;
    let index = path_from_git(index.strip_suffix(b"\n").unwrap_or(&index));
    let modified = async_fs::metadata(index).await?.modified()?;
    let written = modified
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    // Each file's name, then what the index knows of it, indented: `  mtime: <seconds>:<nanos>`.
    let count = entries
        .lines()
        .filter_map(|line| line.strip_prefix("  mtime: "))
        .filter_map(|rest| rest.split_once(':'))
        .filter_map(|(seconds, _)| seconds.parse::<u64>().ok())
        .filter(|seconds| *seconds >= written)
        .count();
    Ok(count)
}

/// A snapshot (see `statusSnapshot`) of the status as the UI got it, or `None` if it didn't.
type Snapshot = Shared<BoxFuture<'static, Option<String>>>;

/// A snapshot (see `statusSnapshot`) of each repository's status as the UI last got it. Git also
/// writes the index when nothing in it changes (another tool's `git status` refreshes it), and
/// after Gitto's own writes the UI already refetches the status itself: comparing the status with
/// the one the UI has tells those apart from a change it hasn't got yet (see `watchGitDir`).
#[derive(Default)]
pub(crate) struct ShownStatuses {
    latest: Mutex<HashMap<PathBuf, (u64, Snapshot)>>,
    set: AtomicU64,
}

impl ShownStatuses {
    /// Remembers `snapshot` as `path`'s; it resolves to `None` if the UI didn't get the status.
    pub fn set(
        &self,
        path: &Path,
        snapshot: impl Future<Output = Option<String>> + Send + 'static,
    ) {
        let number = self.set.fetch_add(1, Ordering::Relaxed);
        lock(&self.latest).insert(path.to_owned(), (number, snapshot.boxed().shared()));
    }

    /// `path`'s latest snapshot, once it's there: a status that's being read is waited for.
    pub async fn get(&self, path: &Path) -> Option<String> {
        loop {
            let (number, latest) = lock(&self.latest).get(path).cloned()?;
            let snapshot = latest.await;
            // Whichever is the latest by then.
            if lock(&self.latest).get(path).map(|(set, _)| *set) == Some(number) {
                return snapshot;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shown_statuses_wait_for_the_latest() {
        smol::block_on(async {
            let statuses = ShownStatuses::default();
            let path = Path::new("/repo");
            assert_eq!(statuses.get(path).await, None);
            statuses.set(path, async { Some("a".to_owned()) });
            assert_eq!(statuses.get(path).await.as_deref(), Some("a"));
            statuses.set(path, async { None });
            assert_eq!(statuses.get(path).await, None);
        });
    }
}
