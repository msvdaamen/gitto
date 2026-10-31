//! Watching the git directory and the working tree. Ported from `commands.ts`.
//!
//! TS's async generators, stopped with an `AbortSignal`, are streams here, stopped by dropping
//! them: what they watch is then unwatched in the background.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use futures::Stream;

use super::batches::Batches;
use super::git_dirs::{classify, inside};
use super::schema::GitDirChange;
use super::stream::{Watched, watch_stream};
use super::watcher::{self, Events, LeaveOut, OnEvents, Root, Subscribe, Subscription};
use crate::core::errors::{Error, Result};
use crate::core::git_dirs::{git_dirs, real_path};
use crate::core::repo::Repo;
use crate::core::runner::{RunOptions, lock};
use crate::core::trace::{trace, tracing};
use crate::features::status::commands::status_snapshot;

/// Folders in the git directory that are big or churn a lot, but never change what the UI shows.
/// `info` has the exclude rules, which only change what shows of the working tree: those are
/// `watch_working_tree`'s to follow.
const GIT_DIR_IGNORED: &[&str] = &["objects", "logs", "hooks", "lfs", "modules", "info"];

/// Yields what changed in the repository's git directory (debounced), until it's dropped.
pub fn watch_git_dir(
    repo: &Repo,
) -> impl Stream<Item = Result<Vec<GitDirChange>>> + Send + Unpin + 'static + use<> {
    watch_git_dir_with(repo.clone(), Arc::new(watcher::subscribe))
}

/// `watch_git_dir`, watching with `subscribe`.
fn watch_git_dir_with(
    repo: Repo,
    subscribe: Subscribe,
) -> impl Stream<Item = Result<Vec<GitDirChange>>> + Send + Unpin + 'static + use<> {
    watch_stream(GitDir::start(repo, subscribe))
}

/// The git directory being watched.
struct GitDir {
    repo: Repo,
    changes: Arc<Batches<GitDirChange>>,
    subscription: Subscription,
}

impl GitDir {
    async fn start(repo: Repo, subscribe: Subscribe) -> Result<GitDir> {
        let dirs = Arc::new(git_dirs(&repo).await?);
        // A linked worktree has its own git directory (inside the shared one) for HEAD and the
        // index.
        let roots = if dirs.git_dir == dirs.common_dir
            || inside(&dirs.common_dir, &dirs.git_dir).is_some()
        {
            vec![dirs.common_dir.clone()]
        } else {
            vec![dirs.common_dir.clone(), dirs.git_dir.clone()]
        };

        let changes = Arc::new(Batches::new());
        let roots = roots
            .into_iter()
            .map(|root| {
                let ignore = GIT_DIR_IGNORED.iter().map(|name| root.join(name)).collect();
                let (changes, dirs) = (Arc::clone(&changes), Arc::clone(&dirs));
                let on_events: OnEvents = Box::new(move |events| match events {
                    Events::Changed(paths) => changes.add_all(
                        paths
                            .iter()
                            .filter_map(|path| classify(&dirs, path))
                            .collect(),
                    ),
                    Events::Rescan => {
                        changes.add_all(vec![GitDirChange::Index, GitDirChange::Refs])
                    }
                    Events::Failed(error) => changes.fail(error),
                });
                Root {
                    dir: root,
                    ignore,
                    ignore_in: &[],
                    leave_out: None,
                    on_events,
                    optional: false,
                }
            })
            .collect();
        let subscription = subscribe(roots).await?;
        Ok(GitDir {
            repo,
            changes,
            subscription,
        })
    }
}

impl Watched for GitDir {
    type Item = Vec<GitDirChange>;

    async fn next(&mut self) -> Option<Result<Vec<GitDirChange>>> {
        while let Some(batch) = self.changes.next().await {
            let unique = match batch {
                Ok(batch) => unique(batch),
                Err(error) => return Some(Err(error)),
            };
            if !unique.contains(&GitDirChange::Refs) && has_status(&self.repo).await {
                trace(format_args!(
                    "index written in {}, with nothing the UI doesn't have",
                    self.repo.path().display()
                ));
                continue;
            }
            if tracing() {
                let names: Vec<&str> = unique.iter().map(|change| change.as_str()).collect();
                trace(format_args!(
                    "git dir changed in {}: {}",
                    self.repo.path().display(),
                    names.join(", ")
                ));
            }
            return Some(Ok(unique));
        }
        None
    }

    async fn stop(self) {
        self.changes.close();
        self.subscription.unsubscribe().await;
    }
}

/// `values` without repeats, in the order they first came (as a JS `Set` keeps them).
fn unique<T: Clone + Eq + std::hash::Hash>(values: Vec<T>) -> Vec<T> {
    let mut seen = HashSet::new();
    values
        .into_iter()
        .filter(|value| seen.insert(value.clone()))
        .collect()
}

/// Whether the UI already has the status as it is after the index was written. It does when
/// nothing in the index changed, as when another tool's `git status` refreshes it, and when it
/// asked for the status since: after staging a file itself, it doesn't wait to hear from the
/// watcher. Reading the status again takes long with many changes, which this doesn't count the
/// lines of.
async fn has_status(repo: &Repo) -> bool {
    let now = status_snapshot(repo).await.ok();
    // Asked for after: a status the UI started on meanwhile, e.g. for a file changing along with
    // the index, is waited for rather than started over.
    now.is_some() && now == repo.shown_status().await
}

/// What changed in the working tree.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum TreeChange {
    /// A path, relative and `/`-separated.
    Path(String),
    /// `.git/info/exclude`, as far as anyone knows.
    Excludes,
    /// Anything: changes went unreported (see `Events::Rescan`).
    Rescan,
}

/// Yields (debounced) whenever a file in the working tree changes, until it's dropped.
pub fn watch_working_tree(
    repo: &Repo,
) -> impl Stream<Item = Result<()>> + Send + Unpin + 'static + use<> {
    watch_working_tree_with(repo.clone(), Arc::new(watcher::subscribe))
}

/// `watch_working_tree`, watching with `subscribe`.
fn watch_working_tree_with(
    repo: Repo,
    subscribe: Subscribe,
) -> impl Stream<Item = Result<()>> + Send + Unpin + 'static + use<> {
    watch_stream(WorkingTree::start(repo, subscribe))
}

/// The working tree being watched.
struct WorkingTree {
    repo: Repo,
    subscribe: Subscribe,
    /// The repository's folder, symlinks resolved, as the watcher reports paths.
    root: PathBuf,
    git_dir: PathBuf,
    /// `.git/info/exclude`, symlinks resolved, which has ignore rules too; `None` if its folder
    /// doesn't exist.
    exclude_file: Option<PathBuf>,
    changes: Arc<Batches<TreeChange>>,
    subscription: Option<Subscription>,
    /// What the current subscription leaves out.
    ignored: HashSet<String>,
    /// The folders git ignores that the current subscription left out since, as they were created
    /// (see `LeaveOut`).
    left_out: Arc<Mutex<HashSet<String>>>,
    /// Ignored paths that changed in the batch last yielded for, to leave out before the next.
    dropped: Vec<String>,
}

impl WorkingTree {
    async fn start(repo: Repo, subscribe: Subscribe) -> Result<WorkingTree> {
        let (root, dirs) = futures::try_join!(real_path(repo.path()), git_dirs(&repo))?;
        // Fine to miss if `info` doesn't exist.
        let exclude_file = match (dirs.exclude_file.parent(), dirs.exclude_file.file_name()) {
            (Some(info), Some(name)) => real_path(info).await.ok().map(|info| info.join(name)),
            _ => None,
        };
        let mut tree = WorkingTree {
            repo,
            subscribe,
            root,
            git_dir: dirs.git_dir,
            exclude_file,
            changes: Arc::new(Batches::new()),
            subscription: None,
            ignored: HashSet::new(),
            left_out: Arc::default(),
            dropped: Vec::new(),
        };
        tree.watch_tree(None).await?;
        Ok(tree)
    }
}

impl Watched for WorkingTree {
    type Item = ();

    async fn next(&mut self) -> Option<Result<()>> {
        // Left out after the change before was yielded, not to hold it back.
        let dropped = std::mem::take(&mut self.dropped);
        if let Err(error) = self.leave_out_new_ignored_folders(&dropped).await {
            return Some(Err(error));
        }

        while let Some(batch) = self.changes.next().await {
            let batch = match batch {
                Ok(batch) => batch,
                Err(error) => return Some(Err(error)),
            };
            let has_excludes = batch.contains(&TreeChange::Excludes);
            let rescan = batch.contains(&TreeChange::Rescan);
            let paths: Vec<String> = unique(batch)
                .into_iter()
                .filter_map(|change| match change {
                    TreeChange::Path(path) => Some(path),
                    TreeChange::Excludes | TreeChange::Rescan => None,
                })
                .collect();
            if has_excludes || paths.iter().any(|path| basename(path) == ".gitignore") {
                trace(format_args!(
                    "ignore rules changed in {}",
                    self.root.display()
                ));
                return Some(self.watch_tree(None).await);
            }
            if rescan {
                // The watcher watches the folders again; only a change to the rules it missed
                // calls for starting over.
                trace(format_args!(
                    "changes went unreported in {}",
                    self.root.display()
                ));
                let now_ignored = ignored_paths(&self.repo).await;
                if now_ignored.len() != self.ignored.len()
                    || now_ignored.iter().any(|path| !self.ignored.contains(path))
                {
                    return Some(self.watch_tree(Some(now_ignored)).await);
                }
                return Some(Ok(()));
            }

            // Only a snapshot of what git ignores is left out, so a folder it ignores that's new
            // since, like fresh build output, is still watched. Changes in it don't show, so
            // they're dropped.
            let dropped = ignored_among(&self.repo, &paths).await;
            let shown: Vec<&str> = paths
                .iter()
                .filter(|path| !dropped.contains(*path))
                .map(String::as_str)
                .collect();
            if tracing() {
                let first: Vec<&str> = shown.iter().take(5).copied().collect();
                let ignored = if dropped.is_empty() {
                    String::new()
                } else {
                    format!(", {} ignored", dropped.len())
                };
                trace(format_args!(
                    "working tree changed in {}: {} files ({}){ignored}",
                    self.root.display(),
                    shown.len(),
                    first.join(", ")
                ));
            }
            let dropped: Vec<String> = dropped.into_iter().collect();
            if !shown.is_empty() {
                self.dropped = dropped;
                return Some(Ok(()));
            }
            if let Err(error) = self.leave_out_new_ignored_folders(&dropped).await {
                return Some(Err(error));
            }
        }
        None
    }

    async fn stop(mut self) {
        self.changes.close();
        if let Some(subscription) = self.subscription.take() {
            subscription.unsubscribe().await;
        }
    }
}

impl WorkingTree {
    /// Watches the tree, starting over if it's being watched, and `.git/info/exclude`.
    ///
    /// Everything git ignores (e.g. `node_modules`) is left out, so it's never walked or watched,
    /// and so is the git directory: that's `watch_git_dir`'s. The ignored paths are a snapshot,
    /// so this starts over whenever the ignore rules change. `paths` are the ignored paths, if
    /// they've just been read.
    async fn watch_tree(&mut self, paths: Option<Vec<String>>) -> Result<()> {
        let now_ignored = match paths {
            Some(paths) => paths,
            None => ignored_paths(&self.repo).await,
        };
        let start = Instant::now();
        let left_out = Arc::default();
        // Started before the one it replaces is stopped, so nothing goes unwatched while it walks
        // the tree. Meanwhile both report what changes in what they both watch, which only adds
        // to the same batch.
        let next = match (self.subscribe)(self.roots(&now_ignored, &left_out)).await {
            Ok(next) => next,
            // Not enough watches for both, as when the tree takes more than half of them: the one it
            // replaces is stopped first, and what changed meanwhile is told as unreported.
            Err(Error::WatchLimitReached) if self.subscription.is_some() => {
                trace(format_args!(
                    "out of watches to start over in {} alongside, so stopping first",
                    self.root.display()
                ));
                if let Some(previous) = self.subscription.take() {
                    previous.unsubscribe().await;
                }
                let next = (self.subscribe)(self.roots(&now_ignored, &left_out)).await?;
                self.changes.add(TreeChange::Rescan);
                next
            }
            Err(error) => return Err(error),
        };
        trace(format_args!(
            "watching {} ({} ignored paths, set up in {:.0}ms)",
            self.root.display(),
            now_ignored.len(),
            start.elapsed().as_secs_f64() * 1000.0
        ));
        if let Some(previous) = self.subscription.replace(next) {
            previous.unsubscribe().await;
        }
        self.ignored = now_ignored.into_iter().collect();
        self.left_out = left_out;
        Ok(())
    }

    /// The tree to watch, but the git directory and `ignored` (as `watch_tree` reads them), and
    /// `.git/info/exclude`. Folders created since that git ignores are left out as they come, and
    /// kept in `left_out`.
    fn roots(&self, ignored: &[String], left_out: &Arc<Mutex<HashSet<String>>>) -> Vec<Root> {
        let ignore = std::iter::once(self.git_dir.clone())
            .chain(ignored.iter().map(|path| self.root.join(path)))
            .collect();
        let (changes, root) = (Arc::clone(&self.changes), self.root.clone());
        let on_events: OnEvents = Box::new(move |events| match events {
            Events::Changed(paths) => changes.add_all(
                paths
                    .iter()
                    .filter_map(|path| inside(&root, path))
                    .map(TreeChange::Path)
                    .collect(),
            ),
            Events::Rescan => changes.add(TreeChange::Rescan),
            Events::Failed(error) => changes.fail(error),
        });
        // Asked on the watcher's thread, before it walks them: a fresh `node_modules` can have
        // tens of thousands of folders.
        let (repo, root, left_out) = (self.repo.clone(), self.root.clone(), Arc::clone(left_out));
        let leave_out: LeaveOut = Box::new(move |folders| {
            let paths: Vec<String> = folders
                .iter()
                .filter_map(|folder| inside(&root, folder))
                .collect();
            let ignored = smol::block_on(ignored_among(&repo, &paths));
            lock(&left_out).extend(ignored.iter().cloned());
            folders
                .iter()
                .filter(|folder| inside(&root, folder).is_some_and(|path| ignored.contains(&path)))
                .cloned()
                .collect()
        });
        let mut roots = vec![Root {
            dir: self.root.clone(),
            ignore,
            // Nested repositories and old-style submodules: what's in them isn't this one's.
            ignore_in: &[".git"],
            leave_out: Some(leave_out),
            on_events,
            optional: false,
        }];
        if let Some(exclude_file) = &self.exclude_file
            && let Some(info) = exclude_file.parent()
        {
            let (changes, exclude_file) = (Arc::clone(&self.changes), exclude_file.clone());
            roots.push(Root {
                dir: info.to_owned(),
                ignore: Vec::new(),
                ignore_in: &[],
                leave_out: None,
                on_events: Box::new(move |events| match events {
                    Events::Changed(paths) if paths.contains(&exclude_file) => {
                        changes.add(TreeChange::Excludes);
                    }
                    // Whether its rules changed is told by what they ignore.
                    Events::Rescan => changes.add(TreeChange::Rescan),
                    _ => {}
                }),
                optional: true,
            });
        }
        roots
    }

    /// Starts over without the folders `dropped` (ignored paths that changed) are, or are in, if
    /// they weren't left out yet, so their changes stop coming in. Not for single files, which are
    /// dropped as they come: they're cheap, and starting over walks the whole tree on Linux. The
    /// folder itself counts: on Linux, a file written right after its folder was created is often
    /// written before the folder is watched, so only the folder is reported. There, the watcher
    /// leaves most new ignored folders out itself (see `roots`), so this only starts over for one
    /// it was told of too late.
    async fn leave_out_new_ignored_folders(&mut self, dropped: &[String]) -> Result<()> {
        if dropped.is_empty() {
            return Ok(());
        }
        let listed = list_ignored(&self.repo).await;
        let left_out = lock(&self.left_out).clone();
        let new_folders: Vec<&str> = listed
            .iter()
            .filter(|path| path.ends_with('/'))
            .map(|path| without_slash(path))
            .filter(|path| {
                !self.ignored.contains(*path)
                    && !left_out.contains(*path)
                    && dropped.iter().any(|changed| {
                        changed == path
                            || changed
                                .strip_prefix(*path)
                                .is_some_and(|rest| rest.starts_with('/'))
                    })
            })
            .collect();
        if new_folders.is_empty() {
            return Ok(());
        }
        trace(format_args!(
            "leaving out new ignored folders in {}: {}",
            self.root.display(),
            new_folders.join(", ")
        ));
        let paths = listed.iter().map(|path| without_slash(path).to_owned());
        self.watch_tree(Some(paths.collect())).await
    }
}

/// The last part of a `/`-separated path.
fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Which of `paths` (relative, `/`-separated) git ignores; tracked files never are.
async fn ignored_among(repo: &Repo, paths: &[String]) -> HashSet<String> {
    if paths.is_empty() {
        return HashSet::new();
    }
    let stdin: String = paths.iter().map(|path| format!("{path}\0")).collect();
    // check-ignore refuses literal pathspecs, but takes its paths as plain paths anyway.
    let checked = repo
        .read_with(
            ["check-ignore", "-z", "--stdin"],
            RunOptions::default().stdin(stdin).glob_pathspecs(),
        )
        .await;
    let output = match checked {
        Ok(output) => output,
        // When none are.
        Err(error) if error.is_exit(1) => String::new(),
        Err(error) => {
            trace(format_args!(
                "couldn't tell what's ignored in {}, so nothing is dropped: {error}",
                repo.path().display()
            ));
            String::new()
        }
    };
    output
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
        .collect()
}

/// What git ignores right now, e.g. `node_modules` or build output: directories an ignore rule
/// matches (listed once) and single files. Relative to the repository, `/`-separated.
///
/// Not `ls-files --ignored --directory`: that also lists a folder that only has ignored files in
/// it so far (a `.DS_Store`, a log), which no rule ignores. Leaving that out, the files added to
/// it later never showed. `--ignored=matching` lists the files in it instead.
pub async fn ignored_paths(repo: &Repo) -> Vec<String> {
    list_ignored(repo)
        .await
        .iter()
        .map(|path| without_slash(path).to_owned())
        .collect()
}

/// `ignored_paths`, with directories ending in `/`.
async fn list_ignored(repo: &Repo) -> Vec<String> {
    // Without renames, each entry is one field; submodules aren't looked into, as only what's
    // ignored here is read.
    let listed = repo
        .read([
            "status",
            "--porcelain=v2",
            "-z",
            "--ignored=matching",
            "--untracked-files=normal",
            "--no-renames",
            "--ignore-submodules=all",
        ])
        .await;
    // Then nothing's left out, and all of it is watched, `node_modules` and all: more watches,
    // but no change missed.
    let output = listed.unwrap_or_else(|error| {
        trace(format_args!(
            "couldn't list what's ignored in {}, so all of it is watched: {error}",
            repo.path().display()
        ));
        String::new()
    });
    output
        .split('\0')
        .filter_map(|entry| entry.strip_prefix("! "))
        .map(str::to_owned)
        .collect()
}

fn without_slash(path: &str) -> &str {
    path.strip_suffix('/').unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::fs;
    use std::path::Path;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::{Duration, SystemTime};

    use async_io::Timer;
    use futures::StreamExt;

    use super::*;
    use crate::features::status::commands::get_status;
    use crate::test_support::{Fixture, git, wait_for};

    use GitDirChange::{Index, Refs};

    /// A stream being watched, as TS's tests hold an async generator: read eagerly in the
    /// background, so it watches whether or not the test is waiting for its next item. Dropping
    /// it stops the stream (TS's `abort`).
    struct Watching<T> {
        items: async_channel::Receiver<T>,
        task: smol::Task<()>,
    }

    fn watching<T: Send + 'static>(
        stream: impl Stream<Item = T> + Send + Unpin + 'static,
    ) -> Watching<T> {
        let (send, items) = async_channel::unbounded();
        let task = smol::spawn(async move {
            let mut stream = stream;
            while let Some(item) = stream.next().await {
                if send.send(item).await.is_err() {
                    break;
                }
            }
        });
        Watching { items, task }
    }

    impl<T> Watching<T> {
        /// The next item; `None` once the stream has ended.
        async fn next(&self) -> Option<T> {
            self.items.recv().await.ok()
        }

        /// The next item, if one comes within `ms`; `None` if it stays quiet for that long.
        async fn within(&self, ms: u64) -> Option<Option<T>> {
            futures_lite::future::or(async { Some(self.next().await) }, async {
                Timer::after(Duration::from_millis(ms)).await;
                None
            })
            .await
        }

        /// Whether no item comes for longer than the watcher's debounce: TS's `quiet(next)`.
        async fn quiet(&self) -> bool {
            self.within(700).await.is_none()
        }

        /// Stops the stream (TS's `abort`), and says whether it ended with nothing more to yield.
        async fn abort(self) -> bool {
            self.task.cancel().await;
            self.items.recv().await.is_err()
        }
    }

    /// The watcher, watched: how many subscriptions were asked for and set up, and everything their
    /// roots were told. Subscribing can be held back, from the `hold_from`th subscription on, until
    /// it's let `go`.
    struct Probe {
        calls: AtomicUsize,
        subscribed: AtomicUsize,
        seen: Mutex<Vec<PathBuf>>,
        /// What the roots are told with, of every subscription.
        roots: Mutex<Vec<Arc<Mutex<OnEvents>>>>,
        hold_from: AtomicUsize,
        gate: (async_channel::Sender<()>, async_channel::Receiver<()>),
        /// How many subscriptions are set up and not yet stopped.
        live: AtomicUsize,
        /// Whether subscribing fails while one is (see `only_one_fits`).
        one_fits: AtomicBool,
    }

    impl Probe {
        fn new() -> Arc<Probe> {
            Arc::new(Probe {
                calls: AtomicUsize::new(0),
                subscribed: AtomicUsize::new(0),
                seen: Mutex::new(Vec::new()),
                roots: Mutex::new(Vec::new()),
                hold_from: AtomicUsize::new(usize::MAX),
                gate: async_channel::unbounded(),
                live: AtomicUsize::new(0),
                one_fits: AtomicBool::new(false),
            })
        }

        fn subscribe(self: &Arc<Self>) -> Subscribe {
            let probe = Arc::clone(self);
            Arc::new(move |roots: Vec<Root>| {
                let probe = Arc::clone(&probe);
                Box::pin(async move {
                    let call = probe.calls.fetch_add(1, Ordering::SeqCst);
                    if call >= probe.hold_from.load(Ordering::SeqCst) {
                        let _ = probe.gate.1.recv().await;
                    }
                    // As if there were only watches enough for one.
                    if probe.one_fits.load(Ordering::SeqCst)
                        && probe.live.load(Ordering::SeqCst) > 0
                    {
                        return Err(Error::WatchLimitReached);
                    }
                    let roots = roots.into_iter().map(|root| {
                        let told = Arc::new(Mutex::new(root.on_events));
                        lock(&probe.roots).push(Arc::clone(&told));
                        let probe = Arc::clone(&probe);
                        let on_events: OnEvents = Box::new(move |events| {
                            if let Events::Changed(paths) = &events {
                                lock(&probe.seen).extend(paths.iter().cloned());
                            }
                            (lock(&told))(events);
                        });
                        Root { on_events, ..root }
                    });
                    let subscription = watcher::subscribe(roots.collect()).await?;
                    probe.subscribed.fetch_add(1, Ordering::SeqCst);
                    probe.live.fetch_add(1, Ordering::SeqCst);
                    let live = Arc::clone(&probe);
                    Ok(Subscription::new(move || {
                        Box::pin(async move {
                            subscription.unsubscribe().await;
                            live.live.fetch_sub(1, Ordering::SeqCst);
                        })
                    }))
                })
            })
        }

        /// Tells every root of every subscription that changes went unreported.
        fn rescan(&self) {
            for told in lock(&self.roots).iter() {
                (lock(told))(Events::Rescan);
            }
        }

        /// Holds subscribing back from the `call`th subscription on (counting from 0).
        fn hold_from(&self, call: usize) {
            self.hold_from.store(call, Ordering::SeqCst);
        }

        /// Has subscribing fail with `Error::WatchLimitReached` while another subscription is set up,
        /// as if there were only watches enough for one.
        fn only_one_fits(&self) {
            self.one_fits.store(true, Ordering::SeqCst);
        }

        /// Lets one subscription that's held back through.
        fn go(&self) {
            self.gate.0.try_send(()).unwrap();
        }

        /// Waits until `count` subscriptions were asked for.
        async fn called(&self, count: usize) {
            let calls = || self.calls.load(Ordering::SeqCst) >= count;
            wait_for(Duration::from_secs(5), calls).await;
        }

        /// Waits until `count` subscriptions are set up.
        async fn subscribed(&self, count: usize) {
            let subscribed = || self.subscribed.load(Ordering::SeqCst) >= count;
            wait_for(Duration::from_secs(5), subscribed).await;
        }

        /// Waits until a root was told `path` changed.
        async fn saw(&self, path: &Path) {
            wait_for(Duration::from_secs(5), || {
                lock(&self.seen).iter().any(|seen| seen == path)
            })
            .await;
        }
    }

    /// The working tree's changes, once it's watched: it only sees what happens after that.
    async fn watched_tree(repo: &Repo) -> Watching<Result<()>> {
        let probe = Probe::new();
        let changes = watching(watch_working_tree_with(repo.clone(), probe.subscribe()));
        probe.subscribed(1).await;
        changes
    }

    /// The git directory's changes, once it's watched: it only sees what happens after that.
    async fn watched_git_dir(repo: &Repo) -> Watching<Result<Vec<GitDirChange>>> {
        let probe = Probe::new();
        let changes = watching(watch_git_dir_with(repo.clone(), probe.subscribe()));
        probe.subscribed(1).await;
        changes
    }

    fn write(path: impl AsRef<Path>, contents: &str) {
        fs::write(path, contents).unwrap();
    }

    fn is_change(item: Option<Result<()>>) -> bool {
        matches!(item, Some(Ok(())))
    }

    fn changed(item: Option<Result<Vec<GitDirChange>>>) -> Vec<GitDirChange> {
        item.expect("a change").expect("no error")
    }

    #[test]
    fn the_working_tree_emits_after_a_change_and_stops_when_aborted() {
        let fx = Fixture::new();
        let path = fx.create_repo("watched");
        smol::block_on(async {
            let repo = fx.repos.open("watched").await.unwrap();
            let changes = watched_tree(&repo).await;

            write(path.join("file.txt"), "x");
            assert!(is_change(changes.next().await));

            assert!(changes.abort().await);
        });
    }

    #[test]
    fn the_working_tree_leaves_the_git_directory_out() {
        let fx = Fixture::new();
        let path = fx.create_repo("watched-git-dir");
        smol::block_on(async {
            let repo = fx.repos.open("watched-git-dir").await.unwrap();
            let changes = watched_tree(&repo).await;

            write(path.join("file.txt"), "x");
            assert!(is_change(changes.next().await));

            git(&path, ["add", "file.txt"]);
            assert!(changes.quiet().await);
        });
    }

    #[test]
    fn the_working_tree_skips_ignored_directories_and_follows_new_ones() {
        let fx = Fixture::new();
        let path = fx.create_repo("watched-tree");
        write(path.join(".gitignore"), "build/\n");
        fs::create_dir(path.join("build")).unwrap();
        smol::block_on(async {
            let repo = fx.repos.open("watched-tree").await.unwrap();
            let changes = watched_tree(&repo).await;

            write(path.join("file.txt"), "x");
            assert!(is_change(changes.next().await));

            // Ignored.
            write(path.join("build").join("out.txt"), "x");
            assert!(changes.quiet().await);

            // Created (and reported) afterwards; changes in it are reported too.
            fs::create_dir(path.join("src")).unwrap();
            assert!(is_change(changes.next().await));
            write(path.join("src").join("a.txt"), "x");
            assert!(is_change(changes.next().await));
        });
    }

    #[test]
    fn the_working_tree_skips_ignored_folders_created_after_it_started() {
        let fx = Fixture::new();
        let path = fx.create_repo("watched-new-ignored");
        write(path.join(".gitignore"), "dist/\n*.log\n");
        smol::block_on(async {
            let repo = fx.repos.open("watched-new-ignored").await.unwrap();
            let changes = watched_tree(&repo).await;

            write(path.join("file.txt"), "x");
            assert!(is_change(changes.next().await));

            // Like a build: a new ignored folder, written to repeatedly. Left out once it's seen,
            // which reports nothing: the tree is watched all the while it starts over.
            fs::create_dir(path.join("dist")).unwrap();
            write(path.join("dist").join("a.js"), "x");
            assert!(changes.quiet().await);
            write(path.join("dist").join("b.js"), "x");
            write(path.join("debug.log"), "x");
            assert!(changes.quiet().await);

            // A change that shows, alongside ignored ones, is still reported.
            write(path.join("dist").join("c.js"), "x");
            write(path.join("file.txt"), "y");
            assert!(is_change(changes.next().await));
        });
    }

    #[test]
    fn the_working_tree_skips_an_ignored_folder_it_only_heard_of_being_created() {
        let fx = Fixture::new();
        let path = fx.create_repo("watched-new-ignored-empty");
        write(path.join(".gitignore"), "dist/\n");
        smol::block_on(async {
            let repo = fx.repos.open("watched-new-ignored-empty").await.unwrap();
            let changes = watched_tree(&repo).await;

            write(path.join("file.txt"), "x");
            assert!(is_change(changes.next().await));

            // As on Linux, where a file written right after its folder is often missed: only the
            // folder is reported. Left out from then on all the same.
            fs::create_dir(path.join("dist")).unwrap();
            assert!(changes.quiet().await);
            write(path.join("dist").join("a.js"), "x");
            assert!(changes.quiet().await);
        });
    }

    #[test]
    fn the_working_tree_leaves_out_a_new_ignored_folder_before_watching_it() {
        let fx = Fixture::new();
        let path = fx.create_repo("watched-new-node-modules");
        write(path.join(".gitignore"), "node_modules/\n");
        let probe = Probe::new();
        smol::block_on(async {
            let repo = fx.repos.open("watched-new-node-modules").await.unwrap();
            let changes = watching(watch_working_tree_with(repo, probe.subscribe()));
            probe.subscribed(1).await;

            // A first `npm install`.
            fs::create_dir_all(path.join("node_modules/a/lib")).unwrap();
            write(path.join("node_modules/a/lib/index.js"), "x");
            assert!(changes.quiet().await);
            write(path.join("node_modules/a/lib/index.js"), "y");
            assert!(changes.quiet().await);
            // On Linux, where it'd walk it, it's left out without starting over.
            if cfg!(target_os = "linux") {
                assert_eq!(probe.calls.load(Ordering::SeqCst), 1);
            }

            write(path.join("file.txt"), "x");
            assert!(is_change(changes.next().await));
        });
    }

    #[test]
    fn the_working_tree_leaves_out_nested_repositories() {
        let fx = Fixture::new();
        let path = fx.create_repo("watched-nested");
        git(&path, ["init", "-q", "nested"]);
        smol::block_on(async {
            let repo = fx.repos.open("watched-nested").await.unwrap();
            let changes = watched_tree(&repo).await;

            // As git does in it.
            git(path.join("nested"), ["config", "user.name", "Nested"]);
            write(path.join("nested").join(".git").join("description"), "x");
            assert!(changes.quiet().await);

            write(path.join("nested").join("file.txt"), "x");
            assert!(is_change(changes.next().await));
        });
    }

    #[test]
    fn the_working_tree_stays_quiet_while_git_reads_it() {
        let fx = Fixture::new();
        let path = fx.create_repo("watched-status");
        for folder in ["a/b/c", "d/e", "f"] {
            fs::create_dir_all(path.join(folder)).unwrap();
            write(path.join(folder).join("file.txt"), "x");
        }
        git(&path, ["add", "."]);
        git(&path, ["commit", "-q", "-m", "First"]);
        smol::block_on(async {
            let repo = fx.repos.open("watched-status").await.unwrap();
            let changes = watched_tree(&repo).await;

            write(path.join("f").join("file.txt"), "y");
            assert!(is_change(changes.next().await));

            // As the UI reads the status after every change: that's no change, nor a rescan.
            for _ in 0..3 {
                git(&path, ["status", "--untracked-files=all"]);
                get_status(&repo).await.unwrap();
            }
            assert!(changes.quiet().await);
        });
    }

    #[test]
    fn the_working_tree_stops_watching_first_when_both_watches_dont_fit() {
        let fx = Fixture::new();
        let path = fx.create_repo("watched-limit");
        let probe = Probe::new();
        smol::block_on(async {
            let repo = fx.repos.open("watched-limit").await.unwrap();
            let changes = watching(watch_working_tree_with(repo, probe.subscribe()));
            probe.subscribed(1).await;

            probe.only_one_fits();
            write(path.join(".gitignore"), "build/\n");
            assert!(is_change(changes.next().await));
            // Then what may have changed while nothing was watched.
            assert!(is_change(changes.next().await));
            assert_eq!(probe.calls.load(Ordering::SeqCst), 3);
            assert_eq!(probe.subscribed.load(Ordering::SeqCst), 2);

            write(path.join("file.txt"), "x");
            assert!(is_change(changes.next().await));
        });
    }

    #[test]
    fn the_working_tree_watches_folders_with_only_ignored_files_in_them() {
        let fx = Fixture::new();
        let path = fx.create_repo("watched-only-ignored");
        write(path.join(".gitignore"), ".DS_Store\n*.log\n");
        fs::create_dir(path.join("docs")).unwrap();
        write(path.join("docs").join(".DS_Store"), "x");
        smol::block_on(async {
            let repo = fx.repos.open("watched-only-ignored").await.unwrap();
            let changes = watched_tree(&repo).await;

            // There from the start: no rule ignores the folder, only what's in it so far.
            write(path.join("docs").join("plan.md"), "x");
            assert!(is_change(changes.next().await));

            // New, with an ignored file first: still watched after that's dropped.
            fs::create_dir(path.join("notes")).unwrap();
            write(path.join("notes").join("debug.log"), "x");
            assert!(is_change(changes.next().await));
            write(path.join("notes").join("todo.md"), "x");
            assert!(is_change(changes.next().await));
        });
    }

    #[test]
    fn the_working_tree_is_watched_while_it_starts_over() {
        let fx = Fixture::new();
        let path = dunce::canonicalize(fx.create_repo("watched-restart")).unwrap();
        let probe = Probe::new();
        smol::block_on(async {
            let repo = fx.repos.open("watched-restart").await.unwrap();
            let changes = watching(watch_working_tree_with(repo, probe.subscribe()));
            probe.subscribed(1).await;

            // Starts over with the new rules, held back from watching anew until the change below
            // was seen: by the watch it replaces.
            probe.hold_from(1);
            write(path.join(".gitignore"), "build/\n");
            probe.called(2).await;
            write(path.join("file.txt"), "x");
            probe.saw(&path.join("file.txt")).await;
            probe.go();
            assert!(is_change(changes.next().await));
            assert!(is_change(changes.next().await));
        });
    }

    #[test]
    fn the_working_tree_reports_a_change_when_changes_went_unreported() {
        let fx = Fixture::new();
        let path = fx.create_repo("watched-rescan");
        let probe = Probe::new();
        smol::block_on(async {
            let repo = fx.repos.open("watched-rescan").await.unwrap();
            let changes = watching(watch_working_tree_with(repo, probe.subscribe()));
            probe.subscribed(1).await;

            probe.rescan();
            assert!(is_change(changes.next().await));
            // With the same rules, it doesn't start over.
            assert_eq!(probe.calls.load(Ordering::SeqCst), 1);

            fs::create_dir(path.join("dist")).unwrap();
            write(path.join("dist").join("a.js"), "x");
            assert!(is_change(changes.next().await));
            // Rules that change unseen: in a file the watch doesn't follow.
            let rules = fx.root().join("rules");
            write(
                &rules, "dist/
",
            );
            git(
                &path,
                [
                    OsStr::new("config"),
                    OsStr::new("core.excludesFile"),
                    rules.as_os_str(),
                ],
            );
            assert!(changes.quiet().await);
            probe.rescan();
            assert!(is_change(changes.next().await));
            assert_eq!(probe.calls.load(Ordering::SeqCst), 2);
        });
    }

    #[test]
    fn the_git_directory_reports_everything_when_changes_went_unreported() {
        let fx = Fixture::new();
        fx.create_repo("watched-git-dir-rescan");
        let probe = Probe::new();
        smol::block_on(async {
            let repo = fx.repos.open("watched-git-dir-rescan").await.unwrap();
            let changes = watching(watch_git_dir_with(repo, probe.subscribe()));
            probe.subscribed(1).await;

            probe.rescan();
            assert_eq!(changed(changes.next().await), [Index, Refs]);
        });
    }

    #[test]
    fn the_working_tree_follows_changes_to_the_ignore_rules() {
        let fx = Fixture::new();
        let path = fx.create_repo("watched-rules");
        write(path.join(".gitignore"), "build/\n");
        fs::create_dir(path.join("build")).unwrap();
        fs::create_dir(path.join("src")).unwrap();
        smol::block_on(async {
            let repo = fx.repos.open("watched-rules").await.unwrap();
            let changes = watched_tree(&repo).await;

            write(path.join("file.txt"), "x");
            assert!(is_change(changes.next().await));

            // `build` is no longer ignored, `src` now is.
            write(path.join(".gitignore"), "src/\n");
            assert!(is_change(changes.next().await));

            write(path.join("build").join("out.txt"), "x");
            assert!(is_change(changes.next().await));

            write(path.join("src").join("a.txt"), "x");
            assert!(changes.quiet().await);

            // Rules in .git/info/exclude count too.
            write(path.join(".git").join("info").join("exclude"), "build/\n");
            assert!(is_change(changes.next().await));
            write(path.join("build").join("out.txt"), "y");
            assert!(changes.quiet().await);
        });
    }

    #[test]
    fn the_git_directory_reports_staging_commits_and_checkouts_but_not_the_working_tree() {
        let fx = Fixture::new();
        let path = fx.create_repo("watched-refs");
        smol::block_on(async {
            let repo = fx.repos.open("watched-refs").await.unwrap();
            let changes = watched_git_dir(&repo).await;

            write(path.join("file.txt"), "x");
            git(&path, ["add", "file.txt"]);
            assert_eq!(changed(changes.next().await), [Index]);

            git(&path, ["commit", "-q", "-m", "First"]);
            assert!(changed(changes.next().await).contains(&Refs));

            git(&path, ["checkout", "-q", "-b", "feature"]);
            assert!(changed(changes.next().await).contains(&Refs));

            write(path.join("file.txt"), "y");
            assert!(changes.quiet().await);

            assert!(changes.abort().await);
        });
    }

    #[test]
    fn the_git_directory_keeps_going_while_the_working_tree_is_watched_and_unwatched() {
        let fx = Fixture::new();
        let path = fx.create_repo("watched-both");
        smol::block_on(async {
            let repo = fx.repos.open("watched-both").await.unwrap();
            let changes = watched_git_dir(&repo).await;

            // Like the UI does when the window gets and loses focus.
            let tree = watched_tree(&repo).await;
            write(path.join("file.txt"), "x");
            assert!(is_change(tree.next().await));
            assert!(tree.abort().await);

            git(&path, ["add", "file.txt"]);
            assert_eq!(changed(changes.next().await), [Index]);
            write(path.join(".git").join("info").join("exclude"), "build/\n");
            git(&path, ["commit", "-q", "-m", "First"]);
            assert!(changed(changes.next().await).contains(&Refs));
        });
    }

    #[test]
    fn the_git_directory_leaves_out_writes_to_the_index_the_ui_has_the_status_of() {
        let fx = Fixture::new();
        let path = fx.create_repo("watched-index");
        let file = path.join("file.txt");
        let index = || fs::read(path.join(".git").join("index")).unwrap();
        write(&file, "x");
        git(&path, ["add", "file.txt"]);
        git(&path, ["commit", "-q", "-m", "First"]);
        smol::block_on(async {
            let repo = fx.repos.open("watched-index").await.unwrap();
            get_status(&repo).await.unwrap();
            let changes = watched_git_dir(&repo).await;

            // Another tool's `git status` refreshes the index after a file was touched: nothing
            // changed.
            let before = index();
            let later = SystemTime::now() + Duration::from_secs(5);
            fs::File::options()
                .write(true)
                .open(&file)
                .unwrap()
                .set_modified(later)
                .unwrap();
            git(&path, ["status"]);
            assert_ne!(index(), before);
            assert!(changes.quiet().await);

            // Staged by the UI, which asks for the status right after rather than waiting for
            // this.
            write(&file, "y");
            git(&path, ["add", "file.txt"]);
            get_status(&repo).await.unwrap();
            assert!(changes.quiet().await);

            // Staged by another tool.
            write(&file, "z");
            git(&path, ["add", "file.txt"]);
            assert_eq!(changed(changes.next().await), [Index]);
        });
    }

    #[test]
    fn the_git_directory_reports_refs_stored_in_a_reftable() {
        // What `git init --ref-format=reftable` (git 2.45+) writes to, instead of `refs`.
        let fx = Fixture::new();
        let path = fx.create_repo("watched-reftable");
        fs::create_dir(path.join(".git").join("reftable")).unwrap();
        smol::block_on(async {
            let repo = fx.repos.open("watched-reftable").await.unwrap();
            let changes = watched_git_dir(&repo).await;

            write(path.join(".git").join("reftable").join("tables.list"), "x");
            assert_eq!(changed(changes.next().await), [Refs]);
        });
    }

    #[test]
    fn the_git_directory_ends_quietly_when_aborted_while_starting() {
        let fx = Fixture::new();
        fx.create_repo("watched-abort");
        smol::block_on(async {
            let repo = fx.repos.open("watched-abort").await.unwrap();
            // Started, and dropped (aborted) before they're done starting.
            let mut git_dir = watch_git_dir(&repo);
            assert!(
                futures_lite::future::poll_once(git_dir.next())
                    .await
                    .is_none()
            );
            drop(git_dir);
            let mut tree = watch_working_tree(&repo);
            assert!(futures_lite::future::poll_once(tree.next()).await.is_none());
            drop(tree);
            // And then end, with nothing more to yield.
            assert!(watching(watch_git_dir(&repo)).abort().await);
            assert!(watching(watch_working_tree(&repo)).abort().await);
        });
    }

    #[test]
    fn the_git_directory_follows_a_linked_worktrees_own_head_and_index() {
        let fx = Fixture::new();
        let main = fx.create_repo("watched-main");
        git(&main, ["commit", "-q", "--allow-empty", "-m", "First"]);
        let linked = fx.root().join("watched-linked");
        git(
            &main,
            [
                OsStr::new("worktree"),
                OsStr::new("add"),
                OsStr::new("-q"),
                linked.as_os_str(),
            ],
        );
        fx.add_path("watched-linked", &linked);
        smol::block_on(async {
            let repo = fx.repos.open("watched-linked").await.unwrap();
            let changes = watched_git_dir(&repo).await;

            write(linked.join("file.txt"), "x");
            git(&linked, ["add", "file.txt"]);
            assert_eq!(changed(changes.next().await), [Index]);

            // The main worktree's index isn't this one's.
            write(main.join("other.txt"), "x");
            git(&main, ["add", "other.txt"]);
            assert!(changes.quiet().await);

            // Branches are shared.
            git(&main, ["branch", "shared"]);
            assert!(changed(changes.next().await).contains(&Refs));
        });
    }
}
