//! What the renderer kept in `localStorage`: small values the UI remembers across restarts, by the
//! same keys (`gitto-theme`, `gitto-panel-width:sidebar`, `gitto-sidebar-collapsed:<id>`…). Kept
//! in a JSON file of its own in the `userData` folder (see `AppPaths::local_storage`).
//!
//! Like `localStorage`, it's read and written synchronously, and a value that can't be saved
//! (the disk is full, say) still applies until the app closes: callers ignore the error, as the
//! hooks did their `try`/`catch`.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;

/// Strings by key, saved to a file on every change (see the module's doc).
#[derive(Debug, Default)]
pub struct LocalStorage {
    /// `None` keeps them in memory only, e.g. in tests.
    file: Option<PathBuf>,
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
            file: Some(file),
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
    pub fn set_item(&self, key: &str, value: impl Into<String>) -> io::Result<()> {
        self.items.borrow_mut().insert(key.to_owned(), value.into());
        self.save()
    }

    pub fn remove_item(&self, key: &str) -> io::Result<()> {
        self.items.borrow_mut().remove(key);
        self.save()
    }

    fn save(&self) -> io::Result<()> {
        let Some(file) = &self.file else {
            return Ok(());
        };
        let text = serde_json::to_string_pretty(&*self.items.borrow()).map_err(io::Error::other)?;
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(file, text)
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
        storage.set_item("gitto-theme", "light").unwrap();

        let reopened = LocalStorage::open(&file);
        assert_eq!(reopened.get_item("gitto-theme").as_deref(), Some("light"));
        reopened.remove_item("gitto-theme").unwrap();
        assert_eq!(LocalStorage::open(&file).get_item("gitto-theme"), None);
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
        assert!(storage.set_item("gitto-theme", "light").is_err());
        assert_eq!(storage.get_item("gitto-theme").as_deref(), Some("light"));
    }
}
