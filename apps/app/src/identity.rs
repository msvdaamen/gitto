//! What the OS knows Gitto by: its id, its name and its icon, where the app tells it these itself.
//! An installer does it for a packaged app; this is what an unpackaged one (`cargo run`) has.
//!
//! - Windows takes the icon and the name from the .exe, which `build.rs` builds them into, and
//!   groups the task bar's windows by the app's id.
//! - macOS takes the Dock's icon from the app bundle, which a build without one doesn't have, so
//!   it's set here as the app starts.
//! - Linux's task bars find an app's name and icon by its desktop entry, named by the window's app
//!   id (Wayland's app id, X11's window class); X11 also takes the window's own icon.

use std::sync::Arc;

use gpui_kit::App;

/// The app's id: its windows' app id on Linux, which names its desktop entry, and its
/// AppUserModelID on Windows. Its own, not the Electron app's (`gitto`), so neither's desktop
/// entry stands in for the other's while both ship.
pub const APP_ID: &str = "io.github.msvdaamen.Gitto";

/// The name the OS shows for the app.
pub const APP_NAME: &str = "Gitto";

/// Gitto's icon, 512px square: the Electron app's `assets/icon.png`.
const ICON_PNG: &[u8] = include_bytes!("../resources/icon.png");

/// Tells the OS who the app is. Call once as it starts, before it opens a window.
pub fn init(cx: &mut App) {
    cx.set_app_identity(APP_ID, APP_NAME);
    #[cfg(target_os = "macos")]
    macos::set_dock_icon();
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    if let Err(error) = linux::register_desktop_entry() {
        log::warn!("Gitto couldn't write its desktop entry, for its name and icon: {error}");
    }
}

/// The window's own icon, which only X11 takes (`_NET_WM_ICON`); `None` elsewhere.
pub fn window_icon() -> Option<Arc<image::RgbaImage>> {
    if !cfg!(any(target_os = "linux", target_os = "freebsd")) {
        return None;
    }
    let icon = image::load_from_memory_with_format(ICON_PNG, image::ImageFormat::Png).ok()?;
    // X11 is sent every pixel, and task bars show it far smaller than 512px.
    let icon = icon.resize(128, 128, image::imageops::FilterType::Lanczos3);
    Some(Arc::new(icon.into_rgba8()))
}

#[cfg(target_os = "macos")]
mod macos {
    use objc2::{AllocAnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    /// Shows Gitto's icon in the Dock, rather than a plain program's.
    pub fn set_dock_icon() {
        let Some(main_thread) = MainThreadMarker::new() else {
            return;
        };
        let data = NSData::with_bytes(super::ICON_PNG);
        let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
            return;
        };
        let app = NSApplication::sharedApplication(main_thread);
        // SAFETY: on the main thread, with an image AppKit made from the PNG.
        unsafe { app.setApplicationIconImage(Some(&image)) };
    }
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
mod linux {
    use std::path::{Path, PathBuf};
    use std::{env, fs, io};

    use super::{APP_ID, APP_NAME, ICON_PNG};

    /// Gives an unpackaged build a desktop entry, so the task bar shows Gitto's name and icon:
    /// on Wayland, a window has no icon of its own, only the app id the desktop finds its entry
    /// by. Kept out of the menus (`NoDisplay`), as it's only a build's. Left to a package that
    /// installs an entry of its own, and only written when it changes.
    pub fn register_desktop_entry() -> io::Result<()> {
        let file = format!("{APP_ID}.desktop");
        let installed = system_data_dirs()
            .iter()
            .any(|dir| dir.join("applications").join(&file).is_file());
        let Some(data_home) = data_home().filter(|_| !installed) else {
            return Ok(());
        };
        let icon = data_home
            .join("icons/hicolor/512x512/apps")
            .join(format!("{APP_ID}.png"));
        write_if_changed(&icon, ICON_PNG)?;
        let entry = desktop_entry(&env::current_exe()?, &icon);
        write_if_changed(&data_home.join("applications").join(file), entry.as_bytes())
    }

    fn desktop_entry(exe: &Path, icon: &Path) -> String {
        format!(
            "[Desktop Entry]\n\
             # Written by Gitto when it runs without being installed, for its name and icon.\n\
             Type=Application\n\
             Name={APP_NAME}\n\
             Comment=Gitto desktop app\n\
             Exec={} %U\n\
             Icon={}\n\
             Categories=Development;RevisionControl;\n\
             StartupWMClass={APP_ID}\n\
             NoDisplay=true\n",
            exec_argument(exe),
            icon.display(),
        )
    }

    /// `path` as an argument of an entry's `Exec`: quoted, escaped as the Desktop Entry spec says
    /// inside quotes, then as any of its strings, and with `%` doubled, as it starts a field code.
    fn exec_argument(path: &Path) -> String {
        let mut quoted = String::from("\"");
        for c in path.display().to_string().chars() {
            if matches!(c, '"' | '`' | '$' | '\\') {
                quoted.push('\\');
            }
            quoted.push(c);
        }
        quoted.push('"');
        quoted.replace('\\', "\\\\").replace('%', "%%")
    }

    /// `$XDG_DATA_HOME`, or `~/.local/share`.
    fn data_home() -> Option<PathBuf> {
        env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|dir| dir.is_absolute())
            .or_else(|| env::home_dir().map(|home| home.join(".local/share")))
    }

    /// `$XDG_DATA_DIRS`, or the spec's default, where packages install their entries.
    fn system_data_dirs() -> Vec<PathBuf> {
        let dirs = env::var("XDG_DATA_DIRS")
            .ok()
            .filter(|dirs| !dirs.is_empty())
            .unwrap_or_else(|| "/usr/local/share:/usr/share".to_owned());
        dirs.split(':')
            .map(PathBuf::from)
            .filter(|dir| dir.is_absolute())
            .collect()
    }

    fn write_if_changed(path: &Path, contents: &[u8]) -> io::Result<()> {
        if fs::read(path).is_ok_and(|current| current == contents) {
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, contents)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn quotes_the_program_for_exec() {
            assert_eq!(
                exec_argument(Path::new("/home/me/my apps/gitto")),
                "\"/home/me/my apps/gitto\""
            );
            assert_eq!(
                exec_argument(Path::new("/a\"b$c`d\\e%f/gitto")),
                "\"/a\\\\\"b\\\\$c\\\\`d\\\\\\\\e%%f/gitto\""
            );
        }

        #[test]
        fn names_the_app_and_its_icon() {
            let entry = desktop_entry(
                Path::new("/opt/gitto/gitto"),
                Path::new("/home/me/.local/share/icons/gitto.png"),
            );
            assert!(entry.starts_with("[Desktop Entry]\n"));
            assert!(entry.contains("\nName=Gitto\n"));
            assert!(entry.contains("\nExec=\"/opt/gitto/gitto\" %U\n"));
            assert!(entry.contains("\nIcon=/home/me/.local/share/icons/gitto.png\n"));
            assert!(entry.contains("\nStartupWMClass=io.github.msvdaamen.Gitto\n"));
        }

        #[test]
        fn writes_only_what_changed() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("applications/gitto.desktop");
            write_if_changed(&path, b"one").unwrap();
            assert_eq!(fs::read(&path).unwrap(), b"one");
            let modified = fs::metadata(&path).unwrap().modified().unwrap();
            write_if_changed(&path, b"one").unwrap();
            assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
            write_if_changed(&path, b"two").unwrap();
            assert_eq!(fs::read(&path).unwrap(), b"two");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_an_icon_for_x11() {
        if cfg!(any(target_os = "linux", target_os = "freebsd")) {
            let icon = window_icon().expect("the icon decodes");
            assert_eq!(icon.dimensions(), (128, 128));
        } else {
            assert!(window_icon().is_none());
        }
    }
}
