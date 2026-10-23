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

/// What a subscription reports: the paths that changed (absolute, under the folder watched), or
/// the error that stopped it.
pub(super) type Events = Result<Vec<PathBuf>>;

/// Told what a subscription reports, from the watcher's thread.
pub(super) type OnEvents = Box<dyn FnMut(Events) + Send + 'static>;

/// Watches the folder `dir` and what's in it but `ignore` (absolute paths), telling `on_events`
/// what changes: TS's `subscribe(dir, callback, { ignore })`.
pub(super) type Subscribe = Arc<
    dyn Fn(PathBuf, Vec<PathBuf>, OnEvents) -> BoxFuture<'static, Result<Subscription>>
        + Send
        + Sync,
>;

/// A folder being watched, until it's unsubscribed, or dropped: dropping it unsubscribes in the
/// background.
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

/// Watches `dir` and what's in it but `ignore`, with `notify` (see the module's docs).
pub(super) fn subscribe(
    dir: PathBuf,
    ignore: Vec<PathBuf>,
    on_events: OnEvents,
) -> BoxFuture<'static, Result<Subscription>> {
    Box::pin(async move {
        // Walking a big tree takes a while.
        let tree = blocking::unblock(move || Tree::watch(dir, ignore, on_events)).await?;
        Ok(Subscription::new(move || {
            Box::pin(blocking::unblock(move || drop(tree)))
        }))
    })
}

/// A tree being watched; dropping it stops the watcher.
struct Tree {
    watcher: Arc<Mutex<Option<RecommendedWatcher>>>,
}

impl Drop for Tree {
    fn drop(&mut self) {
        let watcher = lock(&self.watcher).take();
        drop(watcher);
    }
}

/// The paths a subscription leaves out, and all that's in them.
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

impl Tree {
    fn watch(root: PathBuf, ignore: Vec<PathBuf>, on_events: OnEvents) -> Result<Tree> {
        let ignored = Arc::new(Ignored {
            root: root.clone(),
            paths: ignore.into_iter().collect(),
        });
        let on_events = Arc::new(Mutex::new(on_events));
        // New folders, to watch (from another thread: the watcher's waits for its event handler).
        let (created, new_folders) = mpsc::channel::<PathBuf>();
        let handler = {
            let (ignored, on_events) = (Arc::clone(&ignored), Arc::clone(&on_events));
            move |event: notify::Result<notify::Event>| {
                handle(event, &ignored, &on_events, &created);
            }
        };
        let mut watcher =
            RecommendedWatcher::new(handler, notify::Config::default()).map_err(watch_error)?;
        if WATCHES_TREES {
            watcher
                .watch(&root, RecursiveMode::Recursive)
                .map_err(watch_error)?;
        } else {
            watch_folders(&mut watcher, &root, &ignored, true)?;
        }
        let watcher = Arc::new(Mutex::new(Some(watcher)));
        if !WATCHES_TREES {
            let watcher = Arc::clone(&watcher);
            std::thread::Builder::new()
                .name("gitto-watch".into())
                .spawn(move || {
                    // Ends once the watcher is stopped: its handler, with `created`, is dropped.
                    while let Ok(folder) = new_folders.recv() {
                        let mut watcher = lock(&watcher);
                        let Some(watcher) = watcher.as_mut() else {
                            break;
                        };
                        if let Err(error) = watch_folders(watcher, &folder, &ignored, false) {
                            (lock(&on_events))(Err(error));
                            break;
                        }
                    }
                })?;
        }
        Ok(Tree { watcher })
    }
}

/// Reports what `event` says changed, but for what's ignored, and has new folders watched.
fn handle(
    event: notify::Result<notify::Event>,
    ignored: &Ignored,
    on_events: &Mutex<OnEvents>,
    created: &mpsc::Sender<PathBuf>,
) {
    let event = match event {
        Ok(event) => event,
        Err(error) => return (lock(on_events))(Err(watch_error(error))),
    };
    // Files being opened, read and closed, as git does all the time, change nothing.
    if matches!(event.kind, EventKind::Access(_) | EventKind::Other) {
        return;
    }
    let new = matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Modify(ModifyKind::Name(_))
    );
    let mut paths = Vec::new();
    for path in event.paths {
        if ignored.contains(&path) {
            continue;
        }
        if new
            && !WATCHES_TREES
            && std::fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.is_dir())
        {
            let _ = created.send(path.clone());
        }
        paths.push(path);
    }
    if !paths.is_empty() {
        (lock(on_events))(Ok(paths));
    }
}

/// Watches `folder` and the folders in it, each on its own, but what's ignored. Fails if there
/// are no watches left, or if `folder` can't be watched and it's the `root`; others that can't,
/// like one that's gone since, are left out.
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

/// A watcher's failure, as a file system one: TS's errors from `@parcel/watcher`, uncaught.
fn watch_error(error: notify::Error) -> Error {
    match error.kind {
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

    #[test]
    fn reports_changes_but_not_in_what_it_ignores_and_follows_new_folders() {
        let dir = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(dir.path()).unwrap();
        fs::create_dir(root.join("ignored")).unwrap();
        let seen = Arc::new(Mutex::new(Vec::<PathBuf>::new()));
        let told = Arc::clone(&seen);
        smol::block_on(async {
            let subscription = subscribe(
                root.clone(),
                vec![root.join("ignored")],
                Box::new(move |events| lock(&told).extend(events.unwrap())),
            )
            .await
            .unwrap();
            fs::write(root.join("ignored").join("a"), "x").unwrap();
            fs::write(root.join("a"), "x").unwrap();
            let file = root.join("a");
            wait_for(Duration::from_secs(5), || lock(&seen).contains(&file)).await;

            fs::create_dir(root.join("new")).unwrap();
            let folder = root.join("new");
            wait_for(Duration::from_secs(5), || lock(&seen).contains(&folder)).await;
            async_io::Timer::after(Duration::from_millis(200)).await;
            fs::write(root.join("new").join("b"), "x").unwrap();
            let file = root.join("new").join("b");
            wait_for(Duration::from_secs(5), || lock(&seen).contains(&file)).await;

            subscription.unsubscribe().await;
            assert!(
                !lock(&seen)
                    .iter()
                    .any(|path| path.starts_with(root.join("ignored"))),
                "{:?}",
                lock(&seen)
            );
        });
    }
}
