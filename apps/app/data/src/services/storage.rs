//! What the renderer kept in `localStorage`: small values the UI remembers across restarts, by the
//! same keys (`gitto-theme`, `gitto-panel-width:sidebar`, `gitto-sidebar-collapsed:<id>`…). Kept
//! in a JSON file of its own in the `userData` folder (see `AppPaths::local_storage`).
//!
//! Like `localStorage`, it's read and changed synchronously; the file is written off the UI
//! thread, so the disk never holds a click up. A value that can't be saved (the disk is full,
//! say) still applies until the app closes; why it couldn't is logged, as the hooks' `try`/`catch`
//! ignored it. A file that isn't such JSON (one cut off, say) is moved aside, to
//! `ui-storage.json.bak`, and logged, rather than written over.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;

/// Strings by key, saved to a file on every change (see the module's doc).
#[derive(Debug, Default)]
pub struct LocalStorage {
    /// `None` keeps them in memory only, e.g. in tests.
    writer: Option<Writer>,
    items: RefCell<BTreeMap<String, String>>,
}

impl LocalStorage {
    /// What `file` holds; nothing if there's no file, or it can't be read. One that isn't a JSON
    /// object of strings is moved aside, to a `.bak` beside it, and logged.
    pub fn open(file: impl Into<PathBuf>) -> Self {
        let file = file.into();
        let items = read(&file).unwrap_or_default();
        LocalStorage {
            writer: Some(Writer::start(file)),
            items: RefCell::new(items),
        }
    }

    /// Kept in memory only.
    pub fn in_memory() -> Self {
        LocalStorage::default()
    }

    pub fn get_item(&self, key: &str) -> Option<String> {
        self.items.borrow().get(key).cloned()
    }

    /// Sets `key` to `value`, then saves them all; the value applies even if they can't be.
    pub fn set_item(&self, key: &str, value: impl Into<String>) {
        self.items.borrow_mut().insert(key.to_owned(), value.into());
        self.save();
    }

    pub fn remove_item(&self, key: &str) {
        self.items.borrow_mut().remove(key);
        self.save();
    }

    /// Waits until what's been set is saved; with why the last save failed, if it did. Saves
    /// otherwise finish on their own, and before the storage is dropped.
    pub fn flush(&self) -> io::Result<()> {
        self.writer.as_ref().map_or(Ok(()), Writer::flush)
    }

    fn save(&self) {
        let Some(writer) = &self.writer else {
            return;
        };
        match serde_json::to_string_pretty(&*self.items.borrow()) {
            Ok(text) => writer.write(text),
            Err(error) => log::error!("Couldn't save what the UI remembers: {error}"),
        }
    }
}

/// What `file` holds, if it's there and can be read.
fn read(file: &Path) -> Option<BTreeMap<String, String>> {
    let bytes = match std::fs::read(file) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return None,
        Err(error) => {
            log::warn!(
                "Couldn't read what the UI remembers from {}: {error}",
                file.display()
            );
            return None;
        }
    };
    match serde_json::from_slice(&bytes) {
        Ok(items) => Some(items),
        Err(error) => {
            // Kept, as what's saved next would write over it.
            let aside = backup_path(file);
            let moved = std::fs::rename(file, &aside);
            log::warn!(
                "What the UI remembers in {} isn't readable ({error}); {}",
                file.display(),
                match moved {
                    Ok(()) => format!("it's been moved to {}", aside.display()),
                    Err(error) => format!("it couldn't be moved aside: {error}"),
                }
            );
            None
        }
    }
}

/// Where a file that can't be read is moved: `ui-storage.json.bak`.
fn backup_path(file: &Path) -> PathBuf {
    let mut name = file.file_name().unwrap_or_default().to_owned();
    name.push(".bak");
    file.with_file_name(name)
}

/// Saves the latest of the storage's contents to its file, on a thread of its own: a change made
/// while one's being written replaces any still waiting, so a burst of them is one more write.
#[derive(Debug)]
struct Writer {
    shared: Arc<(Mutex<Pending>, Condvar)>,
    thread: Option<JoinHandle<()>>,
}

#[derive(Debug, Default)]
struct Pending {
    /// What to write next.
    text: Option<String>,
    writing: bool,
    /// Why the last write failed, until `flush` says.
    error: Option<io::Error>,
    closed: bool,
}

impl Writer {
    fn start(file: PathBuf) -> Self {
        let shared = Arc::new((Mutex::new(Pending::default()), Condvar::new()));
        let thread = std::thread::Builder::new()
            .name("local-storage".to_owned())
            .spawn({
                let shared = Arc::clone(&shared);
                move || Writer::run(&file, &shared)
            })
            .expect("a thread to save the UI's storage on");
        Writer {
            shared,
            thread: Some(thread),
        }
    }

    fn run(file: &Path, shared: &(Mutex<Pending>, Condvar)) {
        let (pending, changed) = shared;
        let mut state = lock(pending);
        loop {
            while state.text.is_none() && !state.closed {
                state = changed.wait(state).unwrap_or_else(|e| e.into_inner());
            }
            let Some(text) = state.text.take() else {
                return;
            };
            state.writing = true;
            drop(state);
            let result = write_atomically(file, &text);
            state = lock(pending);
            state.writing = false;
            match result {
                // What was saved replaces what failed to be.
                Ok(()) => state.error = None,
                Err(error) => {
                    log::error!(
                        "Couldn't save what the UI remembers to {}: {error}",
                        file.display()
                    );
                    state.error = Some(error);
                }
            }
            changed.notify_all();
        }
    }

    fn write(&self, text: String) {
        let (pending, changed) = &*self.shared;
        lock(pending).text = Some(text);
        changed.notify_all();
    }

    fn flush(&self) -> io::Result<()> {
        let (pending, changed) = &*self.shared;
        let mut state = lock(pending);
        while state.text.is_some() || state.writing {
            state = changed.wait(state).unwrap_or_else(|e| e.into_inner());
        }
        state.error.take().map_or(Ok(()), Err)
    }
}

impl Drop for Writer {
    /// Writes what's still waiting, then stops.
    fn drop(&mut self) {
        let (pending, changed) = &*self.shared;
        lock(pending).closed = true;
        changed.notify_all();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn lock(pending: &Mutex<Pending>) -> MutexGuard<'_, Pending> {
    pending.lock().unwrap_or_else(|e| e.into_inner())
}

/// Writes `text` beside `file`, then moves it over it: a crash or a power cut halfway through
/// leaves the old file whole, rather than a cut-off one that `open` would read as nothing at all.
fn write_atomically(file: &Path, text: &str) -> io::Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let partial = partial_path(file);
    let written = (|| {
        let mut written = std::fs::File::create(&partial)?;
        written.write_all(text.as_bytes())?;
        // On the disk before it's moved over the old one, not only in the system's cache.
        written.sync_all()?;
        std::fs::rename(&partial, file)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    written
}

/// A file of its own to write `file`'s next contents to: the process's and the write's, as two
/// Gittos running at once (there's nothing to stop that) would otherwise write through the same
/// one, and could move a mix of the two over `file`.
fn partial_path(file: &Path) -> PathBuf {
    static WRITES: AtomicU64 = AtomicU64::new(0);
    let write = WRITES.fetch_add(1, Ordering::Relaxed);
    let mut name = file.file_name().unwrap_or_default().to_owned();
    name.push(format!(".{}-{write}.tmp", std::process::id()));
    file.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_values_across_restarts() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("ui-storage.json");

        let storage = LocalStorage::open(&file);
        assert_eq!(storage.get_item("gitto-theme"), None);
        storage.set_item("gitto-theme", "light");
        storage.flush().unwrap();

        let reopened = LocalStorage::open(&file);
        assert_eq!(reopened.get_item("gitto-theme").as_deref(), Some("light"));
        reopened.remove_item("gitto-theme");
        reopened.flush().unwrap();
        assert_eq!(LocalStorage::open(&file).get_item("gitto-theme"), None);
    }

    #[test]
    fn saves_what_was_set_last_before_its_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("ui-storage.json");

        let storage = LocalStorage::open(&file);
        for width in 200..300 {
            storage.set_item("gitto-panel-width:sidebar", width.to_string());
        }
        drop(storage);

        assert_eq!(
            LocalStorage::open(&file)
                .get_item("gitto-panel-width:sidebar")
                .as_deref(),
            Some("299")
        );
    }

    #[test]
    fn starts_empty_from_an_unreadable_file_and_keeps_it_aside() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("ui-storage.json");
        std::fs::write(&file, "{\"gitto-theme\": \"li").unwrap();

        let storage = LocalStorage::open(&file);
        assert_eq!(storage.get_item("gitto-theme"), None);
        storage.set_item("gitto-panel-width:sidebar", "300");
        storage.flush().unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("ui-storage.json.bak")).unwrap(),
            "{\"gitto-theme\": \"li"
        );
        assert_eq!(
            LocalStorage::open(&file)
                .get_item("gitto-panel-width:sidebar")
                .as_deref(),
            Some("300")
        );
    }

    #[test]
    fn starts_empty_without_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("ui-storage.json");

        assert_eq!(LocalStorage::open(&file).get_item("gitto-theme"), None);
        assert!(!dir.path().join("ui-storage.json.bak").exists());
    }

    #[test]
    fn writes_each_time_through_a_file_of_its_own() {
        let file = Path::new("/data/Gitto/ui-storage.json");
        let (first, second) = (partial_path(file), partial_path(file));
        assert_ne!(first, second);
        assert_eq!(first.parent(), file.parent());
        let name = first.file_name().unwrap().to_str().unwrap();
        assert!(name.starts_with(&format!("ui-storage.json.{}-", std::process::id())));
    }

    #[test]
    fn keeps_a_value_it_cant_save_until_the_app_closes() {
        let dir = tempfile::tempdir().unwrap();
        // A folder where the file should be: it can't be written.
        let file = dir.path().join("ui-storage.json");
        std::fs::create_dir(&file).unwrap();

        let storage = LocalStorage::open(&file);
        storage.set_item("gitto-theme", "light");
        assert!(storage.flush().is_err());
        assert_eq!(storage.get_item("gitto-theme").as_deref(), Some("light"));
        // Nothing's left of the write that failed.
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn says_a_save_failed_only_until_one_works() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("ui-storage.json");
        std::fs::create_dir(&file).unwrap();

        let storage = LocalStorage::open(&file);
        storage.set_item("gitto-theme", "light");
        written(&storage);
        assert!(
            lock(&storage.writer.as_ref().unwrap().shared.0)
                .error
                .is_some()
        );
        std::fs::remove_dir(&file).unwrap();
        storage.set_item("gitto-theme", "dark");
        storage.flush().unwrap();
    }

    /// Waits until what's been set is written, as `flush` does, but leaves why it failed.
    fn written(storage: &LocalStorage) {
        let (pending, changed) = &*storage.writer.as_ref().unwrap().shared;
        let mut state = lock(pending);
        while state.text.is_some() || state.writing {
            state = changed.wait(state).unwrap();
        }
    }
}
