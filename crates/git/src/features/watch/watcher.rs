//! Watching a folder and what's in it, on `notify`: what TS's `@parcel/watcher` `subscribe` did.
//!
//! Like it, this leaves out the paths it's told to ignore: they're never walked or watched, and
//! nothing is reported from them. On Linux (inotify), where each folder is watched on its own,
//! that's done by watching each folder that isn't ignored, and each new one as it's created; on
//! macOS (FSEvents) and Windows, which watch a whole tree at once, by leaving out what's reported
//! from them.

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use futures::future::BoxFuture;
use notify::event::ModifyKind;
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::core::errors::{Error, Result};
use crate::core::runner::{lock, spawn_detached};

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

/// A folder a subscription watches, and what's in it but `ignore` (absolute paths), telling
/// `on_events` what changes in it: TS's `subscribe(dir, callback, { ignore })`.
pub(super) struct Root {
    pub dir: PathBuf,
    pub ignore: Vec<PathBuf>,
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

/// `notify` watches a whole tree at once here, rather than each folder on its own.
const WATCHES_TREES: bool = cfg!(any(target_os = "macos", target_os = "windows"));

/// Watches `roots` with `notify` (see the module's docs): one watcher, so one inotify instance and
/// one thread of notify's on Linux, however many roots.
pub(super) fn subscribe(roots: Vec<Root>) -> BoxFuture<'static, Result<Subscription>> {
    Box::pin(async move {
        // Walking a big tree takes a while.
        let tree = blocking::unblock(move || Tree::watch(roots)).await?;
        Ok(Subscription::new(move || {
            Box::pin(blocking::unblock(move || drop(tree)))
        }))
    })
}

/// The roots being watched; dropping it stops the watcher.
struct Tree {
    watcher: Arc<Mutex<Option<RecommendedWatcher>>>,
}

impl Drop for Tree {
    fn drop(&mut self) {
        let watcher = lock(&self.watcher).take();
        drop(watcher);
    }
}

/// A root's folder, and the paths it leaves out, with all that's in them.
struct Ignored {
    root: PathBuf,
    paths: HashSet<PathBuf>,
}

impl Ignored {
    fn contains(&self, path: &Path) -> bool {
        for ancestor in path.ancestors() {
            if self.paths.contains(ancestor) {
                return true;
            }
            if ancestor == self.root {
                break;
            }
        }
        false
    }
}

/// The roots of a subscription, to tell which one a change is in.
struct Roots {
    /// By the roots' index.
    ignored: Vec<Ignored>,
}

impl Roots {
    /// The index of the root `path` is in: the innermost, if it's in several.
    fn of(&self, path: &Path) -> Option<usize> {
        self.ignored
            .iter()
            .enumerate()
            .filter(|(_, ignored)| path.starts_with(&ignored.root))
            .max_by_key(|(_, ignored)| ignored.root.components().count())
            .map(|(index, _)| index)
    }
}

impl Tree {
    fn watch(roots: Vec<Root>) -> Result<Tree> {
        let mut ignored = Vec::with_capacity(roots.len());
        let mut on_events = Vec::with_capacity(roots.len());
        let mut optional = Vec::with_capacity(roots.len());
        for root in roots {
            ignored.push(Ignored {
                root: root.dir,
                paths: root.ignore.into_iter().collect(),
            });
            on_events.push(root.on_events);
            optional.push(root.optional);
        }
        let roots = Arc::new(Roots { ignored });
        let on_events = Arc::new(Mutex::new(on_events));
        // New folders, to watch (from another thread: the watcher's waits for its event handler),
        // with the index of the root they're in.
        let (created, new_folders) = mpsc::channel::<(usize, PathBuf)>();
        let handler = {
            let (roots, on_events) = (Arc::clone(&roots), Arc::clone(&on_events));
            move |event: notify::Result<notify::Event>| {
                handle(event, &roots, &on_events, &created);
            }
        };
        let mut watcher =
            RecommendedWatcher::new(handler, notify::Config::default()).map_err(watch_error)?;
        for (root, optional) in roots.ignored.iter().zip(optional) {
            if optional && !root.root.is_dir() {
                continue;
            }
            if WATCHES_TREES {
                watcher
                    .watch(&root.root, RecursiveMode::Recursive)
                    .map_err(watch_error)?;
            } else {
                watch_folders(&mut watcher, &root.root, root, true)?;
            }
        }
        let watcher = Arc::new(Mutex::new(Some(watcher)));
        if !WATCHES_TREES {
            let watcher = Arc::clone(&watcher);
            std::thread::Builder::new()
                .name("gitto-watch".into())
                .spawn(move || {
                    // Ends once the watcher is stopped: its handler, with `created`, is dropped.
                    while let Ok((root, folder)) = new_folders.recv() {
                        let mut watcher = lock(&watcher);
                        let Some(watcher) = watcher.as_mut() else {
                            break;
                        };
                        if let Err(error) =
                            watch_folders(watcher, &folder, &roots.ignored[root], false)
                        {
                            tell_all(&on_events, &error);
                            break;
                        }
                    }
                })?;
        }
        Ok(Tree { watcher })
    }
}

/// Tells every root `error`, which stops the watcher for all of them.
fn tell_all(on_events: &Mutex<Vec<OnEvents>>, error: &Error) {
    for on_events in lock(on_events).iter_mut() {
        on_events(Events::Failed(error.clone()));
    }
}

/// Reports what `event` says changed, to the roots it's in, but for what's ignored, and has new
/// folders watched.
fn handle(
    event: notify::Result<notify::Event>,
    roots: &Roots,
    on_events: &Mutex<Vec<OnEvents>>,
    created: &mpsc::Sender<(usize, PathBuf)>,
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
            if !WATCHES_TREES {
                let _ = created.send((root, roots.ignored[root].root.clone()));
            }
            on_events[root](Events::Rescan);
        }
        return;
    }
    // Files being opened, read and closed, as git does all the time, change nothing.
    if matches!(event.kind, EventKind::Access(_) | EventKind::Other) {
        return;
    }
    let new = matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Modify(ModifyKind::Name(_))
    );
    let mut changed: Vec<Vec<PathBuf>> = vec![Vec::new(); roots.ignored.len()];
    for path in event.paths {
        let Some(root) = roots.of(&path) else {
            continue;
        };
        if roots.ignored[root].contains(&path) {
            continue;
        }
        if new
            && !WATCHES_TREES
            && std::fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.is_dir())
        {
            let _ = created.send((root, path.clone()));
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

/// Watches `folder` and the folders in it, each on its own, but what `ignored` leaves out. Fails
/// if there are no watches left, or if `folder` can't be watched and it's the `root`; others that
/// can't, like one that's gone since, are left out.
fn watch_folders(
    watcher: &mut RecommendedWatcher,
    folder: &Path,
    ignored: &Ignored,
    root: bool,
) -> Result<()> {
    let mut folders = vec![(folder.to_owned(), root)];
    while let Some((folder, root)) = folders.pop() {
        if !root && ignored.contains(&folder) {
            continue;
        }
        match watcher.watch(&folder, RecursiveMode::NonRecursive) {
            Ok(()) => {}
            Err(error) if root || matches!(error.kind, notify::ErrorKind::MaxFilesWatch) => {
                return Err(watch_error(error));
            }
            Err(_) => continue,
        }
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            // Not following links, as the watcher doesn't.
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                folders.push((entry.path(), false));
            }
        }
    }
    Ok(())
}

/// A watcher's failure: running out of watches, which the UI can explain, or else a file system
/// one (TS's errors from `@parcel/watcher`, uncaught).
fn watch_error(error: notify::Error) -> Error {
    match error.kind {
        notify::ErrorKind::MaxFilesWatch => Error::WatchLimitReached,
        notify::ErrorKind::Io(error) => Error::from(error),
        _ => Error::from(io::Error::other(error.to_string())),
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
            async_io::Timer::after(Duration::from_millis(200)).await;
            fs::write(root_dir.join("new").join("b"), "x").unwrap();
            let file = root_dir.join("new").join("b");
            wait_for(Duration::from_secs(5), || lock(&seen).contains(&file)).await;

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
                lock(&inner_seen).iter().all(|path| *path == inner_file),
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
    fn running_out_of_watches_is_an_error_of_its_own() {
        let error = watch_error(notify::Error::new(notify::ErrorKind::MaxFilesWatch));
        assert!(matches!(error, Error::WatchLimitReached), "{error:?}");
        assert!(!error.api().is_bug());
    }

    #[test]
    fn tells_every_root_to_rescan_when_changes_went_unreported() {
        let roots = Roots {
            ignored: ["/a", "/b"]
                .into_iter()
                .map(|root| Ignored {
                    root: PathBuf::from(root),
                    paths: HashSet::new(),
                })
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
        let (created, folders) = mpsc::channel();
        // As inotify reports `IN_Q_OVERFLOW`.
        let overflow = notify::Event::new(EventKind::Other).set_flag(notify::event::Flag::Rescan);
        handle(Ok(overflow), &roots, &on_events, &created);
        assert_eq!(
            *lock(&told),
            [(0, "Rescan".to_owned()), (1, "Rescan".to_owned())]
        );
        let rewalked: Vec<_> = folders.try_iter().collect();
        if WATCHES_TREES {
            assert!(rewalked.is_empty());
        } else {
            assert_eq!(
                rewalked,
                [(0, PathBuf::from("/a")), (1, PathBuf::from("/b"))]
            );
        }
    }
}
