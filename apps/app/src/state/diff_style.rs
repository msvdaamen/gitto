//! How a file's changes are laid out. Ported from `hooks/diff-style.ts`.

use crate::services::LocalStorage;

const STORAGE_KEY: &str = "gitto-diff-style";

/// How a file's changes are laid out: in one column, or the old and new side by side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DiffStyle {
    #[default]
    Unified,
    Split,
}

impl DiffStyle {
    pub fn as_str(self) -> &'static str {
        match self {
            DiffStyle::Unified => "unified",
            DiffStyle::Split => "split",
        }
    }

    /// The saved layout: split if it says so, unified otherwise.
    pub fn load(storage: &LocalStorage) -> Self {
        match storage.get_item(STORAGE_KEY).as_deref() {
            Some("split") => DiffStyle::Split,
            _ => DiffStyle::Unified,
        }
    }

    /// Saves it; it still applies if it can't be saved, until the app closes.
    pub fn save(self, storage: &LocalStorage) {
        storage.set_item(STORAGE_KEY, self.as_str());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_unified_until_split_is_saved() {
        let storage = LocalStorage::in_memory();
        assert_eq!(DiffStyle::load(&storage), DiffStyle::Unified);
        DiffStyle::Split.save(&storage);
        assert_eq!(DiffStyle::load(&storage), DiffStyle::Split);
    }
}
