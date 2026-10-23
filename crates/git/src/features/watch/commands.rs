//! Watching the git directory and the working tree. Ported from `commands.ts`.
//!
//! TS's async generators, stopped with an `AbortSignal`, are streams here, stopped by dropping
//! them: what they watch is then unwatched in the background.

use std::collections::HashSet;
use std::future::Future;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock};
use std::time::Instant;

use futures::Stream;
use futures::future::join_all;

use super::batches::Batches;
use super::generator::{Yielder, generate};
use super::git_dirs::{classify, inside};
use super::schema::GitDirChange;
use super::turns::{Turn, Turns};
use super::watcher::{self, OnEvents, Subscribe, Subscription};
use crate::core::errors::Result;
use crate::core::git_dirs::{GitDirs, git_dirs, real_path};
use crate::core::repo::Repo;
use crate::core::runner::RunOptions;
use crate::core::trace::{trace, tracing};
use crate::features::status::commands::status_snapshot;

/// Folders in the git directory that are big or churn a lot, but never change what the UI shows.
/// `info` (with the exclude rules) is `watch_working_tree`'s; the two never watch the same folder.
const GIT_DIR_IGNORED: &[&str] = &["objects", "logs", "hooks", "lfs", "modules", "info"];

static TURNS: LazyLock<Turns> = LazyLock::new(Turns::default);

/// `subscribe`, never while an unsubscribe runs, nor the other way around. The watcher shares what
/// watches (on Linux, one inotify instance) between its subscriptions, and shuts it down with the
/// last unsubscribe. A subscribe that started while that was still removing its watches ends up on
/// the one being shut down, and never hears of a change: after switching from a big repository to
/// another, the new one's git directory wasn't watched, and a commit made in a terminal didn't
/// show.
///
/// That was `@parcel/watcher`'s; `notify` gives each subscription a watcher of its own, but
/// subscribing and unsubscribing still take turns, so a repository that's switched to doesn't
/// wait on one that's left. Its turn is taken now.
fn watch(
    subscribe: &Subscribe,
    dir: PathBuf,
    ignore: Vec<PathBuf>,
    on_events: OnEvents,
) -> impl Future<Output = Result<Subscription>> + Send + 'static + use<> {
    let subscribe = Arc::clone(subscribe);
    let subscribing = TURNS.take(Turn::Subscribe, move || subscribe(dir, ignore, on_events));
    async move {
        let subscription = subscribing.await?;
        Ok(Subscription::new(move || {
            Box::pin(TURNS.take(Turn::Unsubscribe, move || subscription.unsubscribe()))
        }))
    }
}

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
    generate(move |yielder| async move {
        let changes = Arc::new(Batches::new());
        let mut subscriptions = Vec::new();
        let watched = git_dir_changes(&repo, &subscribe, &changes, &mut subscriptions, &yielder);
        let result = watched.await;
        changes.close();
        join_all(subscriptions.into_iter().map(Subscription::unsubscribe)).await;
        if let Err(error) = result {
            yielder.put(Err(error)).await;
        }
    })
}

async fn git_dir_changes(
    repo: &Repo,
    subscribe: &Subscribe,
    changes: &Arc<Batches<GitDirChange>>,
    subscriptions: &mut Vec<Subscription>,
    yielder: &Yielder<Result<Vec<GitDirChange>>>,
) -> Result<()> {
    let dirs = Arc::new(git_dirs(repo).await?);
    // A linked worktree has its own git directory (inside the shared one) for HEAD and the index.
    let roots =
        if dirs.git_dir == dirs.common_dir || inside(&dirs.common_dir, &dirs.git_dir).is_some() {
            vec![dirs.common_dir.clone()]
        } else {
            vec![dirs.common_dir.clone(), dirs.git_dir.clone()]
        };

    let subscribed = join_all(roots.into_iter().map(|root| {
        let ignore = GIT_DIR_IGNORED.iter().map(|name| root.join(name)).collect();
        let (changes, dirs) = (Arc::clone(changes), Arc::clone(&dirs));
        let on_events: OnEvents = Box::new(move |events| match events {
            Err(error) => changes.fail(error),
            Ok(paths) => changes.add_all(
                paths
                    .iter()
                    .filter_map(|path| classify(&dirs, path))
                    .collect(),
            ),
        });
        watch(subscribe, root, ignore, on_events)
    }))
    .await;
    // All the subscriptions that worked are kept (and so unsubscribed), even if one failed.
    let mut failed = None;
    for result in subscribed {
        match result {
            Ok(subscription) => subscriptions.push(subscription),
            Err(error) => {
                failed.get_or_insert(error);
            }
        }
    }
    if let Some(error) = failed {
        return Err(error);
    }

    while let Some(batch) = changes.next().await {
        let unique = unique(batch?);
        if !unique.contains(&GitDirChange::Refs) && has_status(repo).await {
            trace(format_args!(
                "index written in {}, with nothing the UI doesn't have",
                repo.path().display()
            ));
            continue;
        }
        if tracing() {
            let names: Vec<&str> = unique.iter().map(|change| change.as_str()).collect();
            trace(format_args!(
                "git dir changed in {}: {}",
                repo.path().display(),
                names.join(", ")
            ));
        }
        yielder.put(Ok(unique)).await;
    }
    Ok(())
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

/// A changed path in the working tree (relative, `/`-separated), or `.git/info/exclude` changing.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum TreeChange {
    Path(String),
    Excludes,
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
    generate(move |yielder| async move {
        let started = futures::try_join!(real_path(repo.path()), git_dirs(&repo));
        let (root, dirs) = match started {
            Ok(started) => started,
            Err(error) => return yielder.put(Err(error)).await,
        };
        let mut tree = WorkingTree {
            repo,
            subscribe,
            root,
            git_dir: dirs.git_dir.clone(),
            changes: Arc::new(Batches::new()),
            subscription: None,
            ignored: HashSet::new(),
        };
        let mut excludes = None;
        let result = tree.changes(&dirs, &mut excludes, &yielder).await;
        tree.changes.close();
        let unsubscribed = [tree.subscription.take(), excludes]
            .into_iter()
            .flatten()
            .map(Subscription::unsubscribe);
        join_all(unsubscribed).await;
        if let Err(error) = result {
            yielder.put(Err(error)).await;
        }
    })
}

/// The working tree being watched.
struct WorkingTree {
    repo: Repo,
    subscribe: Subscribe,
    /// The repository's folder, symlinks resolved, as the watcher reports paths.
    root: PathBuf,
    git_dir: PathBuf,
    changes: Arc<Batches<TreeChange>>,
    subscription: Option<Subscription>,
    /// What the current subscription leaves out.
    ignored: HashSet<String>,
}

impl WorkingTree {
    async fn changes(
        &mut self,
        dirs: &GitDirs,
        excludes: &mut Option<Subscription>,
        yielder: &Yielder<Result<()>>,
    ) -> Result<()> {
        self.watch_tree(None).await?;
        // `.git/info/exclude` has ignore rules too. Fine to miss if `info` doesn't exist.
        if let Some(info) = dirs.exclude_file.parent() {
            let name = dirs.exclude_file.file_name().map(ToOwned::to_owned);
            let changes = Arc::clone(&self.changes);
            let on_events: OnEvents = Box::new(move |events| {
                if let Ok(paths) = events
                    && paths.iter().any(|path| path.file_name() == name.as_deref())
                {
                    changes.add(TreeChange::Excludes);
                }
            });
            *excludes = watch(&self.subscribe, info.to_owned(), Vec::new(), on_events)
                .await
                .ok();
        }

        while let Some(batch) = self.changes.next().await {
            let batch = batch?;
            let has_excludes = batch.contains(&TreeChange::Excludes);
            let paths: Vec<String> = unique(batch)
                .into_iter()
                .filter_map(|change| match change {
                    TreeChange::Path(path) => Some(path),
                    TreeChange::Excludes => None,
                })
                .collect();
            if has_excludes || paths.iter().any(|path| basename(path) == ".gitignore") {
                trace(format_args!(
                    "ignore rules changed in {}",
                    self.root.display()
                ));
                self.watch_tree(None).await?;
                yielder.put(Ok(())).await;
                continue;
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
            if !shown.is_empty() {
                yielder.put(Ok(())).await;
            }
            // Nothing was watched while it started over (the new subscription can't start before
            // the old one stops, see `watch_tree`), so what changed meanwhile is refetched, once
            // per new folder.
            if !dropped.is_empty() {
                let dropped: Vec<String> = dropped.into_iter().collect();
                if self.leave_out_new_ignored_folders(&dropped).await? {
                    yielder.put(Ok(())).await;
                }
            }
        }
        Ok(())
    }

    /// Watches the tree, starting over if it's being watched.
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
        // Stopped first: the watcher reuses what an existing subscription on the folder already
        // walked, which would leave out a folder that's no longer ignored, and on Linux would watch
        // every folder in one that's newly ignored. Changes in between are reported anyway:
        // callers yield after.
        if let Some(tree) = self.subscription.take() {
            tree.unsubscribe().await;
        }
        let ignore = std::iter::once(self.git_dir.clone())
            .chain(now_ignored.iter().map(|path| self.root.join(path)))
            .collect();
        // A new callback each time: the watcher shares subscriptions with the same callback and
        // folder.
        let (changes, root) = (Arc::clone(&self.changes), self.root.clone());
        let on_events: OnEvents = Box::new(move |events| match events {
            Err(error) => changes.fail(error),
            Ok(paths) => changes.add_all(
                paths
                    .iter()
                    .filter_map(|path| inside(&root, path))
                    .map(TreeChange::Path)
                    .collect(),
            ),
        });
        let next = watch(&self.subscribe, self.root.clone(), ignore, on_events).await?;
        trace(format_args!(
            "watching {} ({} ignored paths, set up in {:.0}ms)",
            self.root.display(),
            now_ignored.len(),
            start.elapsed().as_secs_f64() * 1000.0
        ));
        self.subscription = Some(next);
        self.ignored = now_ignored.into_iter().collect();
        Ok(())
    }

    /// Starts over without the folders `dropped` (ignored paths that changed) are, or are in, if
    /// they weren't left out yet, so their changes stop coming in. Not for single files, which are
    /// dropped as they come: they're cheap, and starting over walks the whole tree on Linux.
    /// Whether it started over. The folder itself counts: on Linux, a file written right after its
    /// folder was created is often written before the folder is watched, so only the folder is
    /// reported.
    async fn leave_out_new_ignored_folders(&mut self, dropped: &[String]) -> Result<bool> {
        let listed = list_ignored(&self.repo).await;
        let new_folders: Vec<&str> = listed
            .iter()
            .filter(|path| path.ends_with('/'))
            .map(|path| without_slash(path))
            .filter(|path| {
                !self.ignored.contains(*path)
                    && dropped.iter().any(|changed| {
                        changed == path
                            || changed
                                .strip_prefix(*path)
                                .is_some_and(|rest| rest.starts_with('/'))
                    })
            })
            .collect();
        if new_folders.is_empty() {
            return Ok(false);
        }
        trace(format_args!(
            "leaving out new ignored folders in {}: {}",
            self.root.display(),
            new_folders.join(", ")
        ));
        let paths = listed.iter().map(|path| without_slash(path).to_owned());
        self.watch_tree(Some(paths.collect())).await?;
        Ok(true)
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
    // Exits with 1 when none are; on any failure, nothing's dropped. check-ignore refuses literal
    // pathspecs, but takes its paths as plain paths anyway.
    let output = repo
        .read_with(
            ["check-ignore", "-z", "--stdin"],
            RunOptions::default().stdin(stdin).glob_pathspecs(),
        )
        .await
        .unwrap_or_default();
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
    let output = repo
        .read([
            "status",
            "--porcelain=v2",
            "-z",
            "--ignored=matching",
            "--untracked-files=normal",
            "--no-renames",
            "--ignore-submodules=all",
        ])
        .await
        .unwrap_or_default();
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
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, SystemTime};

    use async_io::Timer;
    use futures::StreamExt;

    use super::*;
    use crate::core::runner::lock;
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

    /// Runs `change` once the watcher has had time to start; it only sees what happens after that.
    async fn soon(change: impl FnOnce()) {
        Timer::after(Duration::from_millis(200)).await;
        change();
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
            let changes = watching(watch_working_tree(&repo));

            let (next, ()) =
                futures::join!(changes.next(), soon(|| write(path.join("file.txt"), "x")));
            assert!(is_change(next));

            assert!(changes.abort().await);
        });
    }

    #[test]
    fn the_working_tree_leaves_the_git_directory_out() {
        let fx = Fixture::new();
        let path = fx.create_repo("watched-git-dir");
        smol::block_on(async {
            let repo = fx.repos.open("watched-git-dir").await.unwrap();
            let changes = watching(watch_working_tree(&repo));

            let (next, ()) =
                futures::join!(changes.next(), soon(|| write(path.join("file.txt"), "x")));
            assert!(is_change(next));

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
            let changes = watching(watch_working_tree(&repo));

            let (next, ()) =
                futures::join!(changes.next(), soon(|| write(path.join("file.txt"), "x")));
            assert!(is_change(next));

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
            let changes = watching(watch_working_tree(&repo));

            let (next, ()) =
                futures::join!(changes.next(), soon(|| write(path.join("file.txt"), "x")));
            assert!(is_change(next));

            // Like a build: a new ignored folder, written to repeatedly. Left out once it's seen,
            // which reports once, for what changed while it started over.
            fs::create_dir(path.join("dist")).unwrap();
            write(path.join("dist").join("a.js"), "x");
            assert!(is_change(changes.next().await));
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
            let changes = watching(watch_working_tree(&repo));

            let (next, ()) =
                futures::join!(changes.next(), soon(|| write(path.join("file.txt"), "x")));
            assert!(is_change(next));

            // As on Linux, where a file written right after its folder is often missed: only the
            // folder is reported. Left out from then on all the same.
            fs::create_dir(path.join("dist")).unwrap();
            assert!(is_change(changes.next().await));
            write(path.join("dist").join("a.js"), "x");
            assert!(changes.quiet().await);
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
            let changes = watching(watch_working_tree(&repo));

            // There from the start: no rule ignores the folder, only what's in it so far.
            let (next, ()) = futures::join!(
                changes.next(),
                soon(|| write(path.join("docs").join("plan.md"), "x"))
            );
            assert!(is_change(next));

            // New, with an ignored file first: still watched after that's dropped.
            fs::create_dir(path.join("notes")).unwrap();
            write(path.join("notes").join("debug.log"), "x");
            assert!(is_change(changes.next().await));
            write(path.join("notes").join("todo.md"), "x");
            assert!(is_change(changes.next().await));
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
            let changes = watching(watch_working_tree(&repo));

            let (next, ()) =
                futures::join!(changes.next(), soon(|| write(path.join("file.txt"), "x")));
            assert!(is_change(next));

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
            let changes = watching(watch_git_dir(&repo));

            let (next, ()) = futures::join!(
                changes.next(),
                soon(|| {
                    write(path.join("file.txt"), "x");
                    git(&path, ["add", "file.txt"]);
                })
            );
            assert_eq!(changed(next), [Index]);

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
            let changes = watching(watch_git_dir(&repo));

            // Like the UI does when the window gets and loses focus.
            let tree = watching(watch_working_tree(&repo));
            let (next, ()) =
                futures::join!(tree.next(), soon(|| write(path.join("file.txt"), "x")));
            assert!(is_change(next));
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
            let changes = watching(watch_git_dir(&repo));
            Timer::after(Duration::from_millis(200)).await;

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
            let changes = watching(watch_git_dir(&repo));

            let (next, ()) = futures::join!(
                changes.next(),
                soon(|| write(path.join(".git").join("reftable").join("tables.list"), "x"))
            );
            assert_eq!(changed(next), [Refs]);
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
            let changes = watching(watch_git_dir(&repo));

            let (next, ()) = futures::join!(
                changes.next(),
                soon(|| {
                    write(linked.join("file.txt"), "x");
                    git(&linked, ["add", "file.txt"]);
                })
            );
            assert_eq!(changed(next), [Index]);

            // The main worktree's index isn't this one's.
            write(main.join("other.txt"), "x");
            git(&main, ["add", "other.txt"]);
            assert!(changes.quiet().await);

            // Branches are shared.
            git(&main, ["branch", "shared"]);
            assert!(changed(changes.next().await).contains(&Refs));
        });
    }

    /// A watcher whose unsubscribes take a while, like removing a big repository's watches does
    /// (TS's mock of `@parcel/watcher`).
    #[derive(Default)]
    struct SlowWatcher {
        subscribed: Mutex<Vec<PathBuf>>,
        unsubscribing: AtomicUsize,
        /// The folders subscribed to while an unsubscribe was still running.
        overlapped: Mutex<Vec<PathBuf>>,
    }

    impl SlowWatcher {
        fn subscribe(self: &Arc<Self>) -> Subscribe {
            let watcher = Arc::clone(self);
            Arc::new(move |dir: PathBuf, _ignore, _on_events| {
                let watcher = Arc::clone(&watcher);
                Box::pin(async move {
                    if watcher.unsubscribing.load(Ordering::SeqCst) > 0 {
                        lock(&watcher.overlapped).push(dir.clone());
                    }
                    lock(&watcher.subscribed).push(dir);
                    Ok(Subscription::new(move || {
                        Box::pin(async move {
                            watcher.unsubscribing.fetch_add(1, Ordering::SeqCst);
                            Timer::after(Duration::from_millis(100)).await;
                            watcher.unsubscribing.fetch_sub(1, Ordering::SeqCst);
                        })
                    }))
                })
            })
        }
    }

    #[test]
    fn the_watchers_subscriptions_dont_start_while_another_is_being_removed() {
        let fx = Fixture::new();
        fx.create_repo("turn-left");
        fx.create_repo("turn-shown");
        let watcher = Arc::new(SlowWatcher::default());
        smol::block_on(async {
            let (left, shown) =
                futures::try_join!(fx.repos.open("turn-left"), fx.repos.open("turn-shown"))
                    .unwrap();

            let leaving = watching(watch_git_dir_with(left, watcher.subscribe()));
            wait_for(Duration::from_secs(5), || {
                lock(&watcher.subscribed).len() == 1
            })
            .await;

            // As when switching repositories: the old one's watch stops as the new one's starts.
            // A subscribe during an unsubscribe can be left without events for good (see
            // `watch`).
            assert!(leaving.abort().await);
            let started = watching(watch_git_dir_with(shown, watcher.subscribe()));
            wait_for(Duration::from_secs(5), || {
                lock(&watcher.subscribed).len() == 2
            })
            .await;
            assert!(lock(&watcher.overlapped).is_empty());

            assert!(started.abort().await);
        });
    }
}
