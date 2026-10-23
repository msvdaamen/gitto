//! What of `hooks/*.ts` isn't Solid's signals: their logic, for the app's own state to hold.
//!
//! The hooks that saved what they kept did in `localStorage`, under keys and in the format the
//! [`Storage`] here is given, so the app can keep them anywhere: a file, or the database.

use std::collections::HashMap;
use std::io;

pub mod collapsed;
pub mod panel_width;
pub mod relative_time;

/// Where the hooks keep what they save, by key, as `localStorage` did: strings, which reading
/// may not find, and writing may fail to keep (it's full, say), as both could in the browser.
pub trait Storage {
    /// What was saved under `key`; `None` if nothing was, or it can't be read.
    fn get(&self, key: &str) -> Option<String>;
    /// Saves `value` under `key`.
    fn set(&mut self, key: &str, value: &str) -> io::Result<()>;
}

/// A [`Storage`] kept in memory: for tests, or where nothing is to outlive the app.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemoryStorage(pub HashMap<String, String>);

impl Storage for MemoryStorage {
    fn get(&self, key: &str) -> Option<String> {
        self.0.get(key).cloned()
    }

    fn set(&mut self, key: &str, value: &str) -> io::Result<()> {
        self.0.insert(key.to_owned(), value.to_owned());
        Ok(())
    }
}
