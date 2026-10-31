//! Where Gitto keeps its files: the Electron app's `userData` folder, which both apps share while
//! both ship (see `gitto_db::user_data_dir`), so the Rust app opens the same database, settings
//! and "last seen version" as the Electron one (see `apps/electron/src/main.ts`).

use std::path::{Path, PathBuf};

use gitto_db::Profile;

/// Overrides the folder, e.g. to try the app with a database of its own: `GITTO_USER_DATA_DIR`.
pub const USER_DATA_DIR_VAR: &str = "GITTO_USER_DATA_DIR";

/// The files Gitto keeps in its `userData` folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    user_data: PathBuf,
}

impl AppPaths {
    /// The files in `user_data`.
    pub fn new(user_data: impl Into<PathBuf>) -> Self {
        AppPaths {
            user_data: user_data.into(),
        }
    }

    /// The folder `GITTO_USER_DATA_DIR` names, or the Electron app's: a debug build's is a
    /// development run's (`Gitto-dev`), as `pnpm dev`'s is, so it stays out of the installed app's
    /// profile. `None` if there's no home folder to find it in.
    pub fn resolve() -> Option<Self> {
        if let Some(dir) = std::env::var_os(USER_DATA_DIR_VAR).filter(|dir| !dir.is_empty()) {
            return Some(AppPaths::new(dir));
        }
        let profile = if cfg!(debug_assertions) {
            Profile::Development
        } else {
            Profile::Installed
        };
        gitto_db::user_data_dir(profile).map(AppPaths::new)
    }

    pub fn user_data(&self) -> &Path {
        &self.user_data
    }

    /// The database, `gitto.db` (`openDatabase` in main.ts).
    pub fn database(&self) -> PathBuf {
        self.user_data.join(gitto_db::DATABASE_FILE)
    }

    /// The settings read before the app's ready, like whether to send error reports
    /// (`SETTINGS_FILE` in main.ts).
    pub fn settings(&self) -> PathBuf {
        self.user_data.join("settings.json")
    }

    /// The last version the user saw what changed in (`openChangelog` in main.ts).
    pub fn last_seen_version(&self) -> PathBuf {
        self.user_data.join("last-seen-version")
    }

    /// What the Electron app's renderer kept in `localStorage`: the theme, the panels' widths,
    /// which sidebar sections are collapsed (see `crate::services::storage`). The Rust app's own,
    /// as it can't read Chromium's.
    pub fn local_storage(&self) -> PathBuf {
        self.user_data.join("ui-storage.json")
    }

    /// The Rust app's log, `gitto.log` (see `crate::services::logger`).
    pub fn log_file(&self) -> PathBuf {
        self.user_data.join("gitto.log")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_its_files_in_the_user_data_folder() {
        let paths = AppPaths::new("/data/Gitto");
        assert_eq!(paths.database(), Path::new("/data/Gitto/gitto.db"));
        assert_eq!(paths.settings(), Path::new("/data/Gitto/settings.json"));
        assert_eq!(
            paths.last_seen_version(),
            Path::new("/data/Gitto/last-seen-version")
        );
        assert_eq!(
            paths.local_storage(),
            Path::new("/data/Gitto/ui-storage.json")
        );
        assert_eq!(paths.log_file(), Path::new("/data/Gitto/gitto.log"));
    }
}
