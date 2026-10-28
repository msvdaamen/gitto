//! Where the Electron app keeps its database, so the Rust app opens the same file: `gitto.db` in
//! Electron's `app.getPath("userData")` (see `openDatabase` in `apps/electron/src/main.ts`).
//!
//! Electron sets `userData` to `appData` joined with the app's name, its `productName` ("Gitto",
//! in `apps/electron/package.json`). `appData` is, as Electron's path provider has it:
//!
//! - Linux (and other Unix but macOS): `$XDG_CONFIG_HOME` if set and not empty, else
//!   `~/.config`, with `~` from `$HOME` if set and not empty, else the user's entry in the
//!   password database (Chromium's `GetXDGDirectory` and `GetHomeDir`).
//! - macOS: `~/Library/Application Support` (`NSApplicationSupportDirectory`).
//! - Windows: the roaming AppData folder, `%APPDATA%` (`FOLDERID_RoamingAppData`).
//!
//! A development run (`pnpm dev`) appends `-dev` to `userData`, keeping out of the installed app's.

use std::path::PathBuf;

/// The app's name, Electron's `app.getName()`: `productName` in `apps/electron/package.json`.
pub const PRODUCT_NAME: &str = "Gitto";

/// The database's file in `userData`.
pub const DATABASE_FILE: &str = "gitto.db";

/// Which of the Electron app's profiles: the installed app's, or a development run's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Profile {
    /// A packaged build's: `userData` as Electron sets it.
    Installed,
    /// A development run's (`pnpm dev`): `userData` with `-dev` appended, as `main.ts` does.
    Development,
}

/// Electron's `app.getPath("appData")`: the folder of every app's settings (see the module's doc).
/// `None` if the user's home folder can't be found.
pub fn app_data_dir() -> Option<PathBuf> {
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        xdg_config_home(std::env::var_os("XDG_CONFIG_HOME"), dirs::home_dir)
    }
    // `~/Library/Application Support` on macOS, the roaming AppData folder on Windows.
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    {
        dirs::config_dir()
    }
}

/// Electron's `app.getPath("userData")` for the Electron app, in `profile`: `Gitto` in
/// [`app_data_dir`], or `Gitto-dev`.
pub fn user_data_dir(profile: Profile) -> Option<PathBuf> {
    Some(app_data_dir()?.join(user_data_name(profile)))
}

/// The Electron app's database in `profile`: `gitto.db` in [`user_data_dir`].
pub fn database_path(profile: Profile) -> Option<PathBuf> {
    Some(user_data_dir(profile)?.join(DATABASE_FILE))
}

/// The name of the `userData` folder in `appData`.
fn user_data_name(profile: Profile) -> String {
    match profile {
        Profile::Installed => PRODUCT_NAME.to_owned(),
        Profile::Development => format!("{PRODUCT_NAME}-dev"),
    }
}

/// Chromium's `GetXDGDirectory(env, "XDG_CONFIG_HOME", ".config")`: the variable if it's set and
/// not empty, else `.config` in the home folder. `None` without a home folder, where Chromium falls
/// back to the temporary folder: a database kept there would be cleaned away.
#[cfg(all(unix, not(target_os = "macos")))]
fn xdg_config_home(
    xdg_config_home: Option<std::ffi::OsString>,
    home_dir: impl FnOnce() -> Option<PathBuf>,
) -> Option<PathBuf> {
    match xdg_config_home {
        Some(dir) if !dir.is_empty() => Some(PathBuf::from(dir)),
        _ => Some(home_dir()?.join(".config")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn uses_xdg_config_home_when_it_is_set() {
        let dir = xdg_config_home(Some("/custom/config".into()), || {
            Some(PathBuf::from("/home/me"))
        });
        assert_eq!(dir, Some(PathBuf::from("/custom/config")));
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn uses_dot_config_in_the_home_folder_when_xdg_config_home_is_unset_or_empty() {
        let home = || Some(PathBuf::from("/home/me"));
        assert_eq!(
            xdg_config_home(None, home),
            Some(PathBuf::from("/home/me/.config"))
        );
        assert_eq!(
            xdg_config_home(Some("".into()), home),
            Some(PathBuf::from("/home/me/.config"))
        );
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn has_none_without_a_home_folder() {
        assert_eq!(xdg_config_home(None, || None), None);
    }

    #[test]
    fn names_the_user_data_folder_after_the_product() {
        assert_eq!(user_data_name(Profile::Installed), "Gitto");
        assert_eq!(user_data_name(Profile::Development), "Gitto-dev");
    }

    #[test]
    fn puts_the_database_in_user_data() {
        let Some(app_data) = app_data_dir() else {
            return;
        };
        assert_eq!(
            user_data_dir(Profile::Installed).unwrap(),
            app_data.join("Gitto")
        );
        assert_eq!(
            database_path(Profile::Installed).unwrap(),
            app_data.join("Gitto").join("gitto.db")
        );
        assert_eq!(
            database_path(Profile::Development).unwrap(),
            app_data.join("Gitto-dev").join("gitto.db")
        );
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn finds_app_data_as_electron_does_on_linux() {
        let expected = match std::env::var_os("XDG_CONFIG_HOME") {
            Some(dir) if !dir.is_empty() => PathBuf::from(dir),
            _ => dirs::home_dir().unwrap().join(".config"),
        };
        assert_eq!(app_data_dir().unwrap(), expected);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn finds_app_data_as_electron_does_on_macos() {
        assert_eq!(
            app_data_dir().unwrap(),
            dirs::home_dir()
                .unwrap()
                .join("Library/Application Support")
        );
    }

    #[cfg(windows)]
    #[test]
    fn finds_app_data_as_electron_does_on_windows() {
        // Set by Windows for every user session: the roaming AppData folder.
        let appdata = PathBuf::from(std::env::var_os("APPDATA").unwrap());
        assert_eq!(app_data_dir().unwrap(), appdata);
    }
}
