//! Watching a folder and what's in it: what TS's `@parcel/watcher` `subscribe` did. On Linux with
//! inotify directly (see `super::linux`), elsewhere on `notify`.
//!
//! Like it, this leaves out the paths it's told to ignore: they're never walked or watched, and
//! nothing is reported from them. On Linux (inotify), where each folder is watched on its own,
//! that's done by watching each folder that isn't ignored, and each new one as it's created; on
//! macOS (FSEvents) and Windows, which watch a whole tree at once, by leaving out what's reported
//! from them.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use futures::future::BoxFuture;

use crate::core::errors::{Error, Result};
use crate::core::runner::{lock, spawn_detached};

#[cfg(target_os = "linux")]
pub(super) use super::linux::subscribe;

/// What a subscription reports of one of its roots.
#[derive(Debug)]
pub(super) enum Events {
    /// The paths that changed: absolute, under the root's folder.
    Changed(Vec<PathBuf>),
    /// Changes went unreported, e.g. as the kernel's queue of them overflowed (inotify's
    /// `IN_Q_OVERFLOW`) while a big checkout or a build wrote thousands of files: anything in the
    /// root may have changed. The folders in it are watched again, for those created meanwhile.
    Rescan,
    /// The error that stopped the watcher.
    Failed(Error),
}

/// Told what a subscription reports of one of its roots, from the watcher's thread.
pub(super) type OnEvents = Box<dyn FnMut(Events) + Send + 'static>;

/// Asked which of the folders created in a root since it was watched (absolute paths) to leave
/// out, with all that's in them, before they're walked and watched; from the watcher's thread,
/// which waits for it. Only on Linux, where new folders are walked.
pub(super) type LeaveOut = Box<dyn FnMut(&[PathBuf]) -> Vec<PathBuf> + Send + 'static>;

/// A folder a subscription watches, and what's in it but `ignore` (absolute paths), telling
/// `on_events` what changes in it: TS's `subscribe(dir, callback, { ignore })`.
pub(super) struct Root {
    pub dir: PathBuf,
    pub ignore: Vec<PathBuf>,
    /// The names of folders, anywhere in it, whose contents are left out as `ignore`'s are, e.g.
    /// `.git` for nested repositories. The folder itself still counts.
    pub ignore_in: &'static [&'static str],
    /// Asked about new folders, if it's given (see `LeaveOut`).
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub leave_out: Option<LeaveOut>,
    pub on_events: OnEvents,
    /// Whether the subscription goes without it if it isn't there, e.g. as it was deleted.
    pub optional: bool,
}

/// Watches the `Root`s, with one watcher for all of them. A change in a root that's in another is
/// the inner one's.
pub(super) type Subscribe =
    Arc<dyn Fn(Vec<Root>) -> BoxFuture<'static, Result<Subscription>> + Send + Sync>;

/// What a subscription watches, until it's unsubscribed, or dropped: dropping it unsubscribes in
/// the background.
pub(super) struct Subscription {
    stop: Option<Box<dyn FnOnce() -> BoxFuture<'static, ()> + Send>>,
}

impl Subscription {
    /// A subscription that `stop` ends; `stop` is called once, when it's unsubscribed or dropped.
    pub fn new(stop: impl FnOnce() -> BoxFuture<'static, ()> + Send + 'static) -> Self {
        Subscription {
            stop: Some(Box::new(stop)),
        }
    }

    /// Stops watching. Started now; resolves once it's stopped.
    pub fn unsubscribe(mut self) -> BoxFuture<'static, ()> {
        match self.stop.take() {
            Some(stop) => stop(),
            None => Box::pin(async {}),
        }
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            spawn_detached(stop());
        }
    }
}

/// A root's folder, and the paths it leaves out, with all that's in them.
pub(super) struct Ignored {
    pub root: PathBuf,
    paths: HashSet<PathBuf>,
    /// New folders left out since it was watched (see `LeaveOut`).
    left_out: Mutex<HashSet<PathBuf>>,
    /// See `Root::ignore_in`.
    ignore_in: &'static [&'static str],
}

impl Ignored {
    pub fn new(root: PathBuf, paths: Vec<PathBuf>, ignore_in: &'static [&'static str]) -> Self {
        Ignored {
            root,
            paths: paths.into_iter().collect(),
            left_out: Mutex::new(HashSet::new()),
            ignore_in,
        }
    }

    /// Whether `path` is left out: it's ignored, or in a folder that is, or in one whose contents
    /// are (see `Root::ignore_in`).
    pub fn contains(&self, path: &Path) -> bool {
        let left_out = lock(&self.left_out);
        for (depth, ancestor) in path.ancestors().enumerate() {
            if self.paths.contains(ancestor) || left_out.contains(ancestor) {
                return true;
            }
            if ancestor == self.root {
                break;
            }
            if depth > 0 && self.ignores_in(ancestor) {
                return true;
            }
        }
        false
    }

    /// Whether the folder `folder` isn't walked into: it's left out, or only its contents are.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub fn skips(&self, folder: &Path) -> bool {
        self.contains(folder) || (folder != self.root && self.ignores_in(folder))
    }

    /// Leaves out the folders `folders`, new since it was watched, with all that's in them.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub fn leave_out(&self, folders: &[PathBuf]) {
        lock(&self.left_out).extend(folders.iter().cloned());
    }

    fn ignores_in(&self, folder: &Path) -> bool {
        let name = folder.file_name();
        self.ignore_in
            .iter()
            .any(|ignored| name == Some(OsStr::new(ignored)))
    }
}

/// The roots of a subscription, to tell which one a change is in.
pub(super) struct Roots {
    /// By the roots' index.
    pub ignored: Vec<Ignored>,
}

impl Roots {
    /// The index of the root `path` is in: the innermost, if it's in several.
    pub fn of(&self, path: &Path) -> Option<usize> {
        self.ignored
            .iter()
            .enumerate()
            .filter(|(_, ignored)| path.starts_with(&ignored.root))
            .max_by_key(|(_, ignored)| ignored.root.components().count())
            .map(|(index, _)| index)
    }
}

/// Tells every root `error`, which stops the watcher for all of them.
pub(super) fn tell_all(on_events: &Mutex<Vec<OnEvents>>, error: &Error) {
    for on_events in lock(on_events).iter_mut() {
        on_events(Events::Failed(error.clone()));
    }
}

#[cfg(not(target_os = "linux"))]
pub(super) use with_notify::subscribe;

/// Watching each root as a whole tree, with `notify`: FSEvents on macOS, `ReadDirectoryChangesW`
/// on Windows.
#[cfg(not(target_os = "linux"))]
mod with_notify {
    use std::io;
    use std::sync::{Arc, Mutex};

    use futures::future::BoxFuture;
    use notify::event::ModifyKind;
    use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

    use super::{Events, Ignored, OnEvents, Root, Roots, Subscription, tell_all};
    use crate::core::errors::{Error, Result};
    use crate::core::runner::lock;

    /// Watches `roots` with `notify` (see the module's docs): one watcher, however many roots.
    pub(in super::super) fn subscribe(
        roots: Vec<Root>,
    ) -> BoxFuture<'static, Result<Subscription>> {
        Box::pin(async move {
            let tree = blocking::unblock(move || Tree::watch(roots)).await?;
            Ok(Subscription::new(move || {
                Box::pin(blocking::unblock(move || drop(tree)))
            }))
        })
    }

    /// The roots being watched; dropping it stops the watcher.
    struct Tree {
        _watcher: RecommendedWatcher,
    }

    impl Tree {
        fn watch(roots: Vec<Root>) -> Result<Tree> {
            let mut ignored = Vec::with_capacity(roots.len());
            let mut on_events = Vec::with_capacity(roots.len());
            let mut optional = Vec::with_capacity(roots.len());
            for root in roots {
                // New folders aren't walked, so there's nothing to ask about them.
                ignored.push(Ignored::new(root.dir, root.ignore, root.ignore_in));
                on_events.push(root.on_events);
                optional.push(root.optional);
            }
            let roots = Arc::new(Roots { ignored });
            let on_events = Arc::new(Mutex::new(on_events));
            let handler = {
                let roots = Arc::clone(&roots);
                move |event: notify::Result<notify::Event>| handle(event, &roots, &on_events)
            };
            let mut watcher =
                RecommendedWatcher::new(handler, notify::Config::default()).map_err(watch_error)?;
            for (root, optional) in roots.ignored.iter().zip(optional) {
                if optional && !root.root.is_dir() {
                    continue;
                }
                watcher
                    .watch(&root.root, RecursiveMode::Recursive)
                    .map_err(watch_error)?;
            }
            Ok(Tree { _watcher: watcher })
        }
    }

    /// Reports what `event` says changed, to the roots it's in, but for what's ignored.
    pub(super) fn handle(
        event: notify::Result<notify::Event>,
        roots: &Roots,
        on_events: &Mutex<Vec<OnEvents>>,
    ) {
        let event = match event {
            Ok(event) => event,
            Err(error) => return tell_all(on_events, &watch_error(error)),
        };
        // For the whole watcher, as notify reports it, without paths: every root's, then.
        if event.need_rescan() {
            let mut rescanned: Vec<usize> = event
                .paths
                .iter()
                .filter_map(|path| roots.of(path))
                .collect();
            if rescanned.is_empty() {
                rescanned = (0..roots.ignored.len()).collect();
            }
            rescanned.sort_unstable();
            rescanned.dedup();
            let mut on_events = lock(on_events);
            for root in rescanned {
                on_events[root](Events::Rescan);
            }
            return;
        }
        // Files being opened, read and closed, as git does all the time, change nothing.
        if matches!(event.kind, EventKind::Access(_) | EventKind::Other) {
            return;
        }
        // A folder is told modified when what's in it changes, which is told of on its own, ignored
        // or not: `.git` when git writes its index, say. Renamed, it counts.
        let modified =
            matches!(event.kind, EventKind::Modify(kind) if !matches!(kind, ModifyKind::Name(_)));
        let mut changed: Vec<Vec<_>> = vec![Vec::new(); roots.ignored.len()];
        for path in event.paths {
            let Some(root) = roots.of(&path) else {
                continue;
            };
            if roots.ignored[root].contains(&path) || modified && path.is_dir() {
                continue;
            }
            changed[root].push(path);
        }
        let mut on_events = lock(on_events);
        for (paths, on_events) in changed.into_iter().zip(on_events.iter_mut()) {
            if !paths.is_empty() {
                on_events(Events::Changed(paths));
            }
        }
    }

    /// A watcher's failure: running out of watches, which the UI can explain, or else a file
    /// system one (TS's errors from `@parcel/watcher`, uncaught).
    pub(super) fn watch_error(error: notify::Error) -> Error {
        match error.kind {
            notify::ErrorKind::MaxFilesWatch => Error::WatchLimitReached,
            notify::ErrorKind::Io(error) => Error::from(error),
            _ => Error::from(io::Error::other(error.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::Duration;

    use super::*;
    use crate::test_support::wait_for;

    type Seen = Arc<Mutex<Vec<PathBuf>>>;

    /// A root that keeps what it's told in the list it returns.
    fn root(dir: &Path, ignore: Vec<PathBuf>) -> (Root, Seen) {
        let seen = Seen::default();
        let told = Arc::clone(&seen);
        let root = Root {
            dir: dir.to_owned(),
            ignore,
            ignore_in: &[],
            leave_out: None,
            on_events: Box::new(move |events| match events {
                Events::Changed(paths) => lock(&told).extend(paths),
                events => panic!("{events:?}"),
            }),
            optional: false,
        };
        (root, seen)
    }

    #[test]
    fn reports_changes_but_not_in_what_it_ignores_and_follows_new_folders() {
        let dir = tempfile::tempdir().unwrap();
        let root_dir = dunce::canonicalize(dir.path()).unwrap();
        fs::create_dir(root_dir.join("ignored")).unwrap();
        let (watched, seen) = root(&root_dir, vec![root_dir.join("ignored")]);
        smol::block_on(async {
            let subscription = subscribe(vec![watched]).await.unwrap();
            fs::write(root_dir.join("ignored").join("a"), "x").unwrap();
            fs::write(root_dir.join("a"), "x").unwrap();
            let file = root_dir.join("a");
            wait_for(Duration::from_secs(5), || lock(&seen).contains(&file)).await;

            fs::create_dir(root_dir.join("new")).unwrap();
            let folder = root_dir.join("new");
            wait_for(Duration::from_secs(5), || lock(&seen).contains(&folder)).await;
            // Watched once another thread got to it: written again until it's seen.
            let file = root_dir.join("new").join("b");
            wait_for(Duration::from_secs(5), || {
                fs::write(&file, "x").unwrap();
                lock(&seen).contains(&file)
            })
            .await;

            subscription.unsubscribe().await;
            assert!(
                !lock(&seen)
                    .iter()
                    .any(|path| path.starts_with(root_dir.join("ignored"))),
                "{:?}",
                lock(&seen)
            );
        });
    }

    #[test]
    fn tells_each_root_what_changed_in_it_and_an_inner_one_what_changed_in_both() {
        let dir = tempfile::tempdir().unwrap();
        let outer_dir = dunce::canonicalize(dir.path()).unwrap();
        let (ignored_dir, inner_dir) = (outer_dir.join("ignored"), outer_dir.join("ignored/inner"));
        fs::create_dir_all(&inner_dir).unwrap();
        let (outer, outer_seen) = root(&outer_dir, vec![ignored_dir.clone()]);
        let (inner, inner_seen) = root(&inner_dir, Vec::new());
        let (missing, _) = root(&outer_dir.join("missing"), Vec::new());
        let missing = Root {
            optional: true,
            ..missing
        };
        smol::block_on(async {
            let subscription = subscribe(vec![outer, inner, missing]).await.unwrap();
            let (outer_file, inner_file) = (outer_dir.join("a"), inner_dir.join("b"));
            fs::write(ignored_dir.join("c"), "x").unwrap();
            fs::write(&inner_file, "x").unwrap();
            fs::write(&outer_file, "x").unwrap();
            wait_for(Duration::from_secs(5), || {
                lock(&outer_seen).contains(&outer_file) && lock(&inner_seen).contains(&inner_file)
            })
            .await;
            subscription.unsubscribe().await;
            assert!(
                // FSEvents tells of the folder whose files changed, too.
                lock(&inner_seen)
                    .iter()
                    .all(|path| *path == inner_file || *path == inner_dir),
                "{:?}",
                lock(&inner_seen)
            );
            assert!(
                lock(&outer_seen)
                    .iter()
                    .all(|path| !path.starts_with(&ignored_dir)),
                "{:?}",
                lock(&outer_seen)
            );
        });
    }

    #[test]
    fn fails_without_a_root_that_isnt_optional() {
        let dir = tempfile::tempdir().unwrap();
        let (missing, _) = root(&dir.path().join("missing"), Vec::new());
        assert!(smol::block_on(subscribe(vec![missing])).is_err());
    }

    #[test]
    fn leaves_out_what_is_in_folders_by_the_names_its_told() {
        let dir = tempfile::tempdir().unwrap();
        let root_dir = dunce::canonicalize(dir.path()).unwrap();
        // A nested repository's.
        let nested = root_dir.join("nested").join(".git");
        fs::create_dir_all(nested.join("objects")).unwrap();
        let (watched, seen) = root(&root_dir, Vec::new());
        let watched = Root {
            ignore_in: &[".git"],
            ..watched
        };
        smol::block_on(async {
            let subscription = subscribe(vec![watched]).await.unwrap();
            fs::write(nested.join("HEAD"), "x").unwrap();
            fs::write(nested.join("objects").join("a"), "x").unwrap();
            // After those, so they'd be seen first.
            let file = root_dir.join("nested").join("a");
            fs::write(&file, "x").unwrap();
            wait_for(Duration::from_secs(5), || lock(&seen).contains(&file)).await;

            subscription.unsubscribe().await;
            assert!(
                !lock(&seen)
                    .iter()
                    .any(|path| path.starts_with(&nested) && *path != nested),
                "{:?}",
                lock(&seen)
            );
        });
    }

    // What inotify is asked to tell (see `linux`). FSEvents, on macOS, also tells of what changed
    // just before the watch started, which this test would count.
    #[cfg(target_os = "linux")]
    #[test]
    fn hears_nothing_while_git_reads_it() {
        let dir = tempfile::tempdir().unwrap();
        let root_dir = dunce::canonicalize(dir.path()).unwrap();
        crate::test_support::git(&root_dir, ["init", "-q"]);
        for folder in ["a/b/c", "d/e", "f"] {
            fs::create_dir_all(root_dir.join(folder)).unwrap();
            fs::write(root_dir.join(folder).join("file"), "x").unwrap();
        }
        // `root` panics when it's told to rescan.
        let (watched, seen) = root(&root_dir, vec![root_dir.join(".git")]);
        smol::block_on(async {
            let subscription = subscribe(vec![watched]).await.unwrap();
            for _ in 0..3 {
                crate::test_support::git(&root_dir, ["status", "--untracked-files=all"]);
            }
            fs::write(root_dir.join("last"), "x").unwrap();
            let last = root_dir.join("last");
            wait_for(Duration::from_secs(5), || lock(&seen).contains(&last)).await;
            subscription.unsubscribe().await;
            assert!(
                lock(&seen).iter().all(|path| *path == last),
                "{:?}",
                lock(&seen)
            );
        });
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn running_out_of_watches_is_an_error_of_its_own() {
        let error = with_notify::watch_error(notify::Error::new(notify::ErrorKind::MaxFilesWatch));
        assert!(matches!(error, Error::WatchLimitReached), "{error:?}");
        assert!(!error.api().is_bug());
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn tells_every_root_to_rescan_when_changes_went_unreported() {
        let roots = Roots {
            ignored: ["/a", "/b"]
                .into_iter()
                .map(|root| Ignored::new(PathBuf::from(root), Vec::new(), &[]))
                .collect(),
        };
        let told = Arc::new(Mutex::new(Vec::new()));
        let on_events: Vec<OnEvents> = (0..2)
            .map(|root| {
                let told = Arc::clone(&told);
                let on_events: OnEvents =
                    Box::new(move |events| lock(&told).push((root, format!("{events:?}"))));
                on_events
            })
            .collect();
        let on_events = Mutex::new(on_events);
        let overflow =
            notify::Event::new(notify::EventKind::Other).set_flag(notify::event::Flag::Rescan);
        with_notify::handle(Ok(overflow), &roots, &on_events);
        assert_eq!(
            *lock(&told),
            [(0, "Rescan".to_owned()), (1, "Rescan".to_owned())]
        );
    }
}
