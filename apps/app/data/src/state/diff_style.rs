//! How a file's changes are laid out. Ported from `hooks/diff-style.ts`.

use gpui_kit::{App, Global};

use crate::services::{LocalStorage, Services};

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
        let _ = storage.set_item(STORAGE_KEY, self.as_str());
    }

    /// The layout every file's changes are shown in (`useDiffStyle`'s `diffStyle`): the saved one,
    /// read the first time it's asked for.
    pub fn current(cx: &mut App) -> DiffStyle {
        if let Some(setting) = cx.try_global::<DiffStyleSetting>() {
            return setting.0;
        }
        let style = DiffStyle::load(Services::global(cx).storage());
        cx.set_global(DiffStyleSetting(style));
        style
    }

    /// Shows every file's changes in this layout, and saves it (`setDiffStyle`). Views showing
    /// it observe [`DiffStyleSetting`].
    pub fn apply(self, cx: &mut App) {
        cx.set_global(DiffStyleSetting(self));
        self.save(Services::global(cx).storage());
    }
}

/// The layout of the changes on show, shared by every view (see [`DiffStyle::current`]).
pub struct DiffStyleSetting(DiffStyle);

impl Global for DiffStyleSetting {}

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
