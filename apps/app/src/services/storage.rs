//! What the renderer kept in `localStorage`: small values the UI remembers across restarts, by the
//! same keys (`gitto-theme`, `gitto-panel-width:sidebar`, `gitto-sidebar-collapsed:<id>`…). Kept
//! in a JSON file of its own in the `userData` folder (see `AppPaths::local_storage`).
//!
//! Like `localStorage`, it's read and changed synchronously; the file is written off the UI
//! thread, so the disk never holds a click up. A value that can't be saved (the disk is full,
//! say) still applies until the app closes; why it couldn't is logged, as the hooks' `try`/`catch`
//! ignored it.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
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
    /// What `file` holds; nothing if it can't be read, or isn't a JSON object of strings.
    pub fn open(file: impl Into<PathBuf>) -> Self {
        let file = file.into();
        let items = std::fs::read_to_string(&file)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
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
            if let Err(error) = result {
                log::error!(
                    "Couldn't save what the UI remembers to {}: {error}",
                    file.display()
                );
                state.error = Some(error);
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
    let partial = file.with_extension("json.tmp");
    let mut written = std::fs::File::create(&partial)?;
    written.write_all(text.as_bytes())?;
    // On the disk before it's moved over the old one, not only in the system's cache.
    written.sync_all()?;
    std::fs::rename(&partial, file)
}

/// What gitto-ui's hooks (`gitto_ui::hooks`) save what they keep in: this, as the TS hooks did
/// `localStorage`.
impl gitto_ui::hooks::Storage for &LocalStorage {
    fn get(&self, key: &str) -> Option<String> {
        self.get_item(key)
    }

    /// The value applies at once, and is saved off the UI thread, which logs it if it can't be.
    fn set(&mut self, key: &str, value: &str) -> io::Result<()> {
        self.set_item(key, value);
        Ok(())
    }
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
    fn starts_empty_from_an_unreadable_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("ui-storage.json");
        std::fs::write(&file, "not json").unwrap();

        assert_eq!(LocalStorage::open(&file).get_item("gitto-theme"), None);
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
    }
}
