//! Watching on Linux, with inotify directly, as `@parcel/watcher` did: each folder on its own, for
//! what parcel watched for (see `super::watcher` for the rest).
//!
//! Not with notify, whose inotify backend always watches for folders and files being opened too
//! (`IN_OPEN`), and can't be told otherwise. git opens every folder it reads, on every `git status`:
//! thousands of events in a big repository, enough to overflow the kernel's queue of them, which
//! reads as changes going unreported, which has the status read again.
//!
//! One thread reads the events, and another walks the folders created meanwhile, to watch them.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::io::{self, PipeReader, PipeWriter};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};

use futures::future::BoxFuture;
use inotify::{Event, EventMask, Inotify, WatchDescriptor, WatchMask, Watches};

use super::watcher::{Events, Ignored, LeaveOut, OnEvents, Root, Roots, Subscription, tell_all};
use crate::core::errors::{Error, Result};
use crate::core::runner::lock;

/// What's watched for: what's in a folder being created, deleted, moved, written to or having its
/// permissions changed, and the folder itself being deleted or moved; not anything being opened,
/// read or closed. Only folders are watched, links aren't followed, and a file deleted while it's
/// still open isn't reported on as it's written to.
const WATCH_MASK: WatchMask = WatchMask::ATTRIB
    .union(WatchMask::CREATE)
    .union(WatchMask::DELETE)
    .union(WatchMask::DELETE_SELF)
    .union(WatchMask::MODIFY)
    .union(WatchMask::MOVE_SELF)
    .union(WatchMask::MOVED_FROM)
    .union(WatchMask::MOVED_TO)
    .union(WatchMask::DONT_FOLLOW)
    .union(WatchMask::ONLYDIR)
    .union(WatchMask::EXCL_UNLINK);

/// How much of the kernel's queue of events is read at once.
const BUFFER_BYTES: usize = 64 * 1024;

/// What the walking thread is asked to watch.
#[derive(Debug, PartialEq, Eq)]
enum Walk {
    /// A folder created in a root (by its index), or moved into it: watched with the folders in
    /// it, unless the root's `LeaveOut` leaves it out.
    New(usize, PathBuf),
    /// All of a root again, as changes went unreported: for the folders created meanwhile.
    Again(usize),
}

/// What the subscription's threads share.
struct Shared {
    roots: Roots,
    on_events: Mutex<Vec<OnEvents>>,
    /// The folder each watch is of.
    folders: Mutex<HashMap<WatchDescriptor, PathBuf>>,
    /// Set once it's stopped, for a walk to stop at the next folder.
    stopped: AtomicBool,
}

/// Watches `roots` with inotify (see the module's docs): one inotify instance, however many roots.
pub(super) fn subscribe(roots: Vec<Root>) -> BoxFuture<'static, Result<Subscription>> {
    Box::pin(async move {
        // Walking a big tree takes a while.
        let tree = blocking::unblock(move || Tree::watch(roots)).await?;
        Ok(Subscription::new(move || {
            Box::pin(blocking::unblock(move || drop(tree)))
        }))
    })
}

/// The roots being watched; dropping it stops the threads, and once they're done, closes the
/// inotify instance, which removes its watches.
struct Tree {
    shared: Arc<Shared>,
    /// Closed to wake the thread reading events, to stop.
    stop: Option<PipeWriter>,
    threads: Vec<JoinHandle<()>>,
}

impl Drop for Tree {
    fn drop(&mut self) {
        self.shared.stopped.store(true, Ordering::Relaxed);
        drop(self.stop.take());
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

impl Tree {
    fn watch(roots: Vec<Root>) -> Result<Tree> {
        let mut ignored = Vec::with_capacity(roots.len());
        let mut on_events = Vec::with_capacity(roots.len());
        let mut leave_out = Vec::with_capacity(roots.len());
        let mut optional = Vec::with_capacity(roots.len());
        for root in roots {
            ignored.push(Ignored::new(root.dir, root.ignore, root.ignore_in));
            on_events.push(root.on_events);
            leave_out.push(root.leave_out);
            optional.push(root.optional);
        }
        let inotify = Inotify::init().map_err(watch_error)?;
        let mut watches = inotify.watches();
        let shared = Arc::new(Shared {
            roots: Roots { ignored },
            on_events: Mutex::new(on_events),
            folders: Mutex::new(HashMap::new()),
            stopped: AtomicBool::new(false),
        });
        let (stopped, stop) = io::pipe()?;
        let mut tree = Tree {
            shared: Arc::clone(&shared),
            stop: Some(stop),
            threads: Vec::with_capacity(2),
        };
        let (walk, walks) = mpsc::channel();
        // Reading from the start, so the kernel's queue doesn't fill while a big tree is walked.
        let reading = {
            let shared = Arc::clone(&shared);
            thread::Builder::new()
                .name("gitto-watch-events".into())
                .spawn(move || read_events(&shared, inotify, &stopped, &walk))?
        };
        tree.threads.push(reading);
        let walking = {
            let (shared, watches) = (Arc::clone(&shared), watches.clone());
            thread::Builder::new()
                .name("gitto-watch".into())
                .spawn(move || walk_new_folders(&shared, watches, leave_out, &walks))?
        };
        tree.threads.push(walking);
        for (root, optional) in optional.into_iter().enumerate() {
            let dir = &shared.roots.ignored[root].root;
            if optional && !dir.is_dir() {
                continue;
            }
            watch_folders(&shared, &mut watches, dir, root, true)?;
        }
        Ok(tree)
    }
}

/// Reads `inotify`'s events as they come and handles them (see `handle`), until `stopped`'s other
/// end is closed.
fn read_events(
    shared: &Shared,
    mut inotify: Inotify,
    stopped: &PipeReader,
    walk: &mpsc::Sender<Walk>,
) {
    let mut buffer = vec![0; BUFFER_BYTES];
    let mut watches = inotify.watches();
    loop {
        let mut fds = [inotify.as_raw_fd(), stopped.as_raw_fd()].map(|fd| libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        });
        // SAFETY: `fds` is an array of two `pollfd`s, as `poll` is told, of file descriptors that
        // stay open while it waits.
        if unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) } < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return tell_all(&shared.on_events, &Error::from(error));
        }
        if fds[1].revents != 0 {
            return;
        }
        match inotify.read_events(&mut buffer) {
            Ok(events) => handle(shared, &mut watches, events, walk),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) => {}
            Err(error) => return tell_all(&shared.on_events, &Error::from(error)),
        }
    }
}

/// Reports what `events`, read at once, say changed, to the roots they're in, but for what's
/// ignored. Has new folders watched, and keeps up with watched folders that are moved or gone.
fn handle<'a>(
    shared: &Shared,
    watches: &mut Watches,
    events: impl IntoIterator<Item = Event<&'a OsStr>>,
    walk: &mpsc::Sender<Walk>,
) {
    let roots = &shared.roots;
    // Not walked into nor watched: what's ignored, or outside every root.
    let skipped = |path: &Path| {
        roots
            .of(path)
            .is_none_or(|root| roots.ignored[root].skips(path))
    };
    let mut changed: Vec<Vec<PathBuf>> = vec![Vec::new(); roots.ignored.len()];
    let mut rescan = false;
    // Folders moved away, by the cookie that pairs each with where it went.
    let mut moved: HashMap<u32, PathBuf> = HashMap::new();
    let mut folders = lock(&shared.folders);
    for event in events {
        if event.mask.contains(EventMask::Q_OVERFLOW) {
            rescan = true;
            continue;
        }
        // The watch is gone, as its folder was deleted, say.
        if event.mask.contains(EventMask::IGNORED) {
            folders.remove(&event.wd);
            continue;
        }
        let Some(folder) = folders.get(&event.wd) else {
            continue;
        };
        let path = match event.name {
            Some(name) => folder.join(name),
            None => folder.clone(),
        };
        let Some(root) = roots.of(&path) else {
            continue;
        };
        // Told by the event: no need to look.
        if event.mask.contains(EventMask::ISDIR) {
            if event.mask.contains(EventMask::MOVED_FROM) {
                moved.insert(event.cookie, path.clone());
            }
            if event
                .mask
                .intersects(EventMask::CREATE | EventMask::MOVED_TO)
            {
                let from = event
                    .mask
                    .contains(EventMask::MOVED_TO)
                    .then(|| moved.remove(&event.cookie))
                    .flatten();
                match from {
                    // Renamed within what's watched: it's still watched, under its new name.
                    Some(from) if !skipped(&from) && !skipped(&path) => {
                        rename(&mut folders, &from, &path);
                    }
                    from => {
                        if let Some(from) = from {
                            unwatch(&mut folders, watches, &from);
                        }
                        if !skipped(&path) {
                            let _ = walk.send(Walk::New(root, path.clone()));
                        }
                    }
                }
            }
        }
        if !roots.ignored[root].contains(&path) {
            changed[root].push(path);
        }
    }
    // Moved out of what's watched, or to where it's yet to be heard of.
    for from in moved.into_values() {
        unwatch(&mut folders, watches, &from);
    }
    drop(folders);
    let mut on_events = lock(&shared.on_events);
    if rescan {
        for (root, on_events) in on_events.iter_mut().enumerate() {
            let _ = walk.send(Walk::Again(root));
            on_events(Events::Rescan);
        }
    }
    for (paths, on_events) in changed.into_iter().zip(on_events.iter_mut()) {
        if !paths.is_empty() {
            on_events(Events::Changed(paths));
        }
    }
}

/// Has the watches of the folder `from`, and of the folders in it, go by where it was moved, `to`.
fn rename(folders: &mut HashMap<WatchDescriptor, PathBuf>, from: &Path, to: &Path) {
    for folder in folders.values_mut() {
        if let Ok(rest) = folder.strip_prefix(from) {
            *folder = if rest.as_os_str().is_empty() {
                to.to_owned()
            } else {
                to.join(rest)
            };
        }
    }
}

/// Stops watching the folder `from`, and the folders in it, which are no longer there.
fn unwatch(folders: &mut HashMap<WatchDescriptor, PathBuf>, watches: &mut Watches, from: &Path) {
    folders.retain(|watch, folder| {
        let gone = folder.starts_with(from);
        if gone {
            let _ = watches.remove(watch.clone());
        }
        !gone
    });
}

/// Watches what `read_events` asks it to, as it comes, until it's stopped. A failure, like running
/// out of watches, is told every root, and stops it.
fn walk_new_folders(
    shared: &Shared,
    mut watches: Watches,
    mut leave_out: Vec<Option<LeaveOut>>,
    walks: &mpsc::Receiver<Walk>,
) {
    // Ends once `read_events` does, with its end.
    while let Ok(first) = walks.recv() {
        // And all that came meanwhile, to ask about the new folders at once.
        let asked: Vec<Walk> = std::iter::once(first).chain(walks.try_iter()).collect();
        if let Err(error) = walk(shared, &mut watches, &mut leave_out, asked) {
            tell_all(&shared.on_events, &error);
            break;
        }
        if shared.stopped.load(Ordering::Relaxed) {
            break;
        }
    }
}

/// Watches the folders `asked`, but the new ones their root's `LeaveOut` leaves out.
fn walk(
    shared: &Shared,
    watches: &mut Watches,
    leave_out: &mut [Option<LeaveOut>],
    asked: Vec<Walk>,
) -> Result<()> {
    let mut new: Vec<Vec<PathBuf>> = vec![Vec::new(); leave_out.len()];
    for walk in asked {
        match walk {
            Walk::Again(root) => {
                let dir = &shared.roots.ignored[root].root;
                watch_folders(shared, watches, dir, root, false)?;
            }
            Walk::New(root, folder) => new[root].push(folder),
        }
    }
    for (root, folders) in new.into_iter().enumerate() {
        if folders.is_empty() {
            continue;
        }
        let ignored = &shared.roots.ignored[root];
        if let Some(leave_out) = &mut leave_out[root] {
            ignored.leave_out(&leave_out(&folders));
        }
        for folder in folders {
            watch_folders(shared, watches, &folder, root, false)?;
        }
    }
    Ok(())
}

/// Watches `folder` and the folders in it, each on its own, but what root `root` leaves out. Fails
/// if there are no watches left, or if `folder` can't be watched and it's `required` (a root's
/// own); others that can't, like one that's gone since, are left out. Stops once it's stopped.
fn watch_folders(
    shared: &Shared,
    watches: &mut Watches,
    folder: &Path,
    root: usize,
    required: bool,
) -> Result<()> {
    let ignored = &shared.roots.ignored[root];
    let mut folders = vec![(folder.to_owned(), required)];
    while let Some((folder, required)) = folders.pop() {
        if shared.stopped.load(Ordering::Relaxed) {
            return Ok(());
        }
        if !required && ignored.skips(&folder) {
            continue;
        }
        {
            // Held while it's added, so its events aren't read before it's known whose they are.
            let mut watched = lock(&shared.folders);
            match watches.add(&folder, WATCH_MASK) {
                Ok(watch) => {
                    watched.insert(watch, folder.clone());
                }
                Err(error) if required || error.raw_os_error() == Some(libc::ENOSPC) => {
                    return Err(watch_error(error));
                }
                Err(_) => continue,
            }
        }
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            // Not following links, as the watches don't.
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                folders.push((entry.path(), false));
            }
        }
    }
    Ok(())
}

/// A watcher's failure: running out of watches (`ENOSPC`), which the UI can explain, or else a
/// file system one (TS's errors from `@parcel/watcher`, uncaught).
fn watch_error(error: io::Error) -> Error {
    if error.raw_os_error() == Some(libc::ENOSPC) {
        Error::WatchLimitReached
    } else {
        Error::from(error)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::Duration;

    use super::*;
    use crate::test_support::wait_for;

    type Told = Arc<Mutex<Vec<String>>>;

    /// A root of `dir` that keeps what it's told, by root `name`, in `told`.
    fn root(dir: &Path, name: &str, told: &Told) -> Root {
        let (name, told) = (name.to_owned(), Arc::clone(told));
        Root {
            dir: dir.to_owned(),
            ignore: Vec::new(),
            ignore_in: &[],
            leave_out: None,
            on_events: Box::new(move |events| lock(&told).push(format!("{name} {events:?}"))),
            optional: false,
        }
    }

    fn temp_dir() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dunce::canonicalize(dir.path()).unwrap();
        (dir, path)
    }

    /// The folders `tree` watches.
    fn watched(tree: &Tree) -> Vec<PathBuf> {
        let mut folders: Vec<PathBuf> = lock(&tree.shared.folders).values().cloned().collect();
        folders.sort();
        folders
    }

    #[test]
    fn hears_nothing_of_folders_and_files_only_being_read() {
        let (_dir, path) = temp_dir();
        fs::create_dir(path.join("folder")).unwrap();
        fs::write(path.join("file"), "x").unwrap();
        let mut inotify = Inotify::init().unwrap();
        inotify.watches().add(&path, WATCH_MASK).unwrap();

        // As `git status` reads every folder.
        for entry in fs::read_dir(&path).unwrap() {
            let _ = entry.unwrap().metadata();
        }
        fs::read_dir(path.join("folder")).unwrap().for_each(drop);
        fs::read(path.join("file")).unwrap();
        let mut buffer = [0; 4096];
        let error = inotify.read_events(&mut buffer).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::WouldBlock, "{error}");
    }

    #[test]
    fn leaves_out_a_new_folder_before_walking_it() {
        let (_dir, path) = temp_dir();
        let told = Told::default();
        let asked = Arc::new(Mutex::new(Vec::new()));
        let leave_out: LeaveOut = {
            let asked = Arc::clone(&asked);
            Box::new(move |folders: &[PathBuf]| {
                lock(&asked).extend(folders.iter().cloned());
                folders
                    .iter()
                    .filter(|folder| folder.ends_with("node_modules"))
                    .cloned()
                    .collect()
            })
        };
        let tree = Tree::watch(vec![Root {
            leave_out: Some(leave_out),
            ..root(&path, "root", &told)
        }])
        .unwrap();

        fs::create_dir_all(path.join("node_modules/a/b")).unwrap();
        fs::create_dir_all(path.join("src/a")).unwrap();
        smol::block_on(wait_for(Duration::from_secs(5), || {
            watched(&tree).contains(&path.join("src/a"))
        }));
        assert_eq!(
            watched(&tree),
            [path.clone(), path.join("src"), path.join("src/a")]
        );
        // Asked before it's walked, so nothing in it is.
        assert!(
            lock(&asked)
                .iter()
                .all(|folder| !folder.starts_with(path.join("node_modules/a"))),
            "{:?}",
            lock(&asked)
        );

        // And nothing in it is reported, as nothing in an ignored folder is.
        fs::write(path.join("node_modules/a/x"), "x").unwrap();
        fs::write(path.join("node_modules/y"), "x").unwrap();
        fs::write(path.join("src/a/z"), "x").unwrap();
        smol::block_on(wait_for(Duration::from_secs(5), || {
            lock(&told).iter().any(|told| told.contains("src/a/z"))
        }));
        drop(tree);
        assert!(
            !lock(&told)
                .iter()
                .any(|told| told.contains("node_modules/")),
            "{:?}",
            lock(&told)
        );
    }

    #[test]
    fn keeps_up_with_folders_that_are_renamed_or_moved_out() {
        let (_dir, path) = temp_dir();
        let outside = tempfile::tempdir().unwrap();
        fs::create_dir_all(path.join("tree/a/b")).unwrap();
        let told = Told::default();
        let tree = Tree::watch(vec![root(&path.join("tree"), "root", &told)]).unwrap();

        fs::rename(path.join("tree/a"), path.join("tree/c")).unwrap();
        smol::block_on(wait_for(Duration::from_secs(5), || {
            watched(&tree).contains(&path.join("tree/c/b"))
        }));
        fs::write(path.join("tree/c/b/x"), "x").unwrap();
        smol::block_on(wait_for(Duration::from_secs(5), || {
            lock(&told).iter().any(|told| told.contains("tree/c/b/x"))
        }));

        let away = dunce::canonicalize(outside.path()).unwrap().join("c");
        fs::rename(path.join("tree/c"), &away).unwrap();
        smol::block_on(wait_for(Duration::from_secs(5), || {
            watched(&tree) == [path.join("tree")]
        }));
        drop(tree);
    }

    #[test]
    fn tells_every_root_to_rescan_when_changes_went_unreported() {
        let (_dir, path) = temp_dir();
        let told = Told::default();
        let shared = Shared {
            roots: Roots {
                ignored: ["a", "b"]
                    .iter()
                    .map(|name| Ignored::new(path.join(name), Vec::new(), &[]))
                    .collect(),
            },
            on_events: Mutex::new(
                ["a", "b"]
                    .iter()
                    .map(|name| root(&path.join(name), name, &told).on_events)
                    .collect(),
            ),
            folders: Mutex::new(HashMap::new()),
            stopped: AtomicBool::new(false),
        };
        let inotify = Inotify::init().unwrap();
        let mut watches = inotify.watches();
        // As inotify reports it, for no watch: any will do here.
        let overflow = Event {
            wd: watches.add(&path, WATCH_MASK).unwrap(),
            mask: EventMask::Q_OVERFLOW,
            cookie: 0,
            name: None,
        };
        let (walk, walks) = mpsc::channel();
        handle(&shared, &mut watches, [overflow], &walk);
        assert_eq!(*lock(&told), ["a Rescan", "b Rescan"]);
        assert_eq!(
            walks.try_iter().collect::<Vec<_>>(),
            [Walk::Again(0), Walk::Again(1)]
        );
    }

    #[test]
    fn running_out_of_watches_is_an_error_of_its_own() {
        let error = watch_error(io::Error::from_raw_os_error(libc::ENOSPC));
        assert!(matches!(error, Error::WatchLimitReached), "{error:?}");
        assert!(!error.api().is_bug());
    }
}
