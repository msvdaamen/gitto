//! Watching the git directory and the working tree. Ported from `commands.ts`.
//!
//! TS's async generators, stopped with an `AbortSignal`, are streams here, stopped by dropping
//! them: what they watch is then unwatched in the background.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use futures::Stream;
use futures::future::join_all;

use super::batches::Batches;
use super::git_dirs::{classify, inside};
use super::schema::GitDirChange;
use super::stream::{Watched, watch_stream};
use super::watcher::{self, OnEvents, Subscribe, Subscription};
use crate::core::errors::Result;
use crate::core::git_dirs::{git_dirs, real_path};
use crate::core::repo::Repo;
use crate::core::runner::RunOptions;
use crate::core::trace::{trace, tracing};
use crate::features::status::commands::status_snapshot;

/// Folders in the git directory that are big or churn a lot, but never change what the UI shows.
/// `info` (with the exclude rules) is `watch_working_tree`'s; the two never watch the same folder.
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
    subscriptions: Vec<Subscription>,
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
        let subscribed = join_all(roots.into_iter().map(|root| {
            let ignore = GIT_DIR_IGNORED.iter().map(|name| root.join(name)).collect();
            let (changes, dirs) = (Arc::clone(&changes), Arc::clone(&dirs));
            let on_events: OnEvents = Box::new(move |events| match events {
                Err(error) => changes.fail(error),
                Ok(paths) => changes.add_all(
                    paths
                        .iter()
                        .filter_map(|path| classify(&dirs, path))
                        .collect(),
                ),
            });
            subscribe(root, ignore, on_events)
        }))
        .await;
        let mut watched = GitDir {
            repo,
            changes,
            subscriptions: Vec::new(),
        };
        // All the subscriptions that worked are kept (and so unsubscribed), even if one failed.
        let mut failed = None;
        for result in subscribed {
            match result {
                Ok(subscription) => watched.subscriptions.push(subscription),
                Err(error) => {
                    failed.get_or_insert(error);
                }
            }
        }
        match failed {
            Some(error) => {
                watched.stop().await;
                Err(error)
            }
            None => Ok(watched),
        }
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
        join_all(
            self.subscriptions
                .into_iter()
                .map(Subscription::unsubscribe),
        )
        .await;
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
    watch_stream(WorkingTree::start(repo, subscribe))
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
    /// The watch of `.git/info`, for changes to `.git/info/exclude`; `None` if it couldn't start.
    excludes: Option<Subscription>,
    /// What the current subscription leaves out.
    ignored: HashSet<String>,
    /// Ignored paths that changed in the batch last yielded for, to leave out before the next.
    dropped: Vec<String>,
}

impl WorkingTree {
    async fn start(repo: Repo, subscribe: Subscribe) -> Result<WorkingTree> {
        let (root, dirs) = futures::try_join!(real_path(repo.path()), git_dirs(&repo))?;
        let mut tree = WorkingTree {
            repo,
            subscribe,
            root,
            git_dir: dirs.git_dir.clone(),
            changes: Arc::new(Batches::new()),
            subscription: None,
            excludes: None,
            ignored: HashSet::new(),
            dropped: Vec::new(),
        };
        if let Err(error) = tree.watch_tree(None).await {
            tree.stop().await;
            return Err(error);
        }
        // `.git/info/exclude` has ignore rules too. Fine to miss if `info` doesn't exist.
        if let Some(info) = dirs.exclude_file.parent() {
            let name = dirs.exclude_file.file_name().map(ToOwned::to_owned);
            let changes = Arc::clone(&tree.changes);
            let on_events: OnEvents = Box::new(move |events| {
                if let Ok(paths) = events
                    && paths.iter().any(|path| path.file_name() == name.as_deref())
                {
                    changes.add(TreeChange::Excludes);
                }
            });
            tree.excludes = (tree.subscribe)(info.to_owned(), Vec::new(), on_events)
                .await
                .ok();
        }
        Ok(tree)
    }
}

impl Watched for WorkingTree {
    type Item = ();

    async fn next(&mut self) -> Option<Result<()>> {
        // Nothing was watched while it started over (the new subscription can't start before the
        // old one stops, see `watch_tree`), so what changed meanwhile is refetched, once per new
        // folder.
        if !self.dropped.is_empty() {
            let dropped = std::mem::take(&mut self.dropped);
            match self.leave_out_new_ignored_folders(&dropped).await {
                Ok(true) => return Some(Ok(())),
                Ok(false) => {}
                Err(error) => return Some(Err(error)),
            }
        }

        while let Some(batch) = self.changes.next().await {
            let batch = match batch {
                Ok(batch) => batch,
                Err(error) => return Some(Err(error)),
            };
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
                return Some(self.watch_tree(None).await);
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
            let has_shown = !shown.is_empty();
            self.dropped = dropped.into_iter().collect();
            if has_shown {
                return Some(Ok(()));
            }
            if !self.dropped.is_empty() {
                let dropped = std::mem::take(&mut self.dropped);
                match self.leave_out_new_ignored_folders(&dropped).await {
                    Ok(true) => return Some(Ok(())),
                    Ok(false) => {}
                    Err(error) => return Some(Err(error)),
                }
            }
        }
        None
    }

    async fn stop(mut self) {
        self.changes.close();
        let unsubscribed = [self.subscription.take(), self.excludes.take()]
            .into_iter()
            .flatten()
            .map(Subscription::unsubscribe);
        join_all(unsubscribed).await;
    }
}

impl WorkingTree {
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
        let next = (self.subscribe)(self.root.clone(), ignore, on_events).await?;
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
    use std::time::{Duration, SystemTime};

    use async_io::Timer;
    use futures::StreamExt;

    use super::*;
    use crate::features::status::commands::get_status;
    use crate::test_support::{Fixture, git};

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
}
