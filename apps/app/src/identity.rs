//! What the OS knows Gitto by: its id, its name and its icon, where the app tells it these itself.
//! An installer does it for a packaged app; this is what an unpackaged one (`cargo run`) has.
//!
//! - Windows takes the icon and the name from the .exe, which `build.rs` builds them into, and
//!   groups the task bar's windows by the app's id.
//! - macOS takes the Dock's icon from the app bundle, which a build without one doesn't have, so
//!   it's set here as the app starts.
//! - Linux's task bars find an app's name and icon by its desktop entry, named by the window's app
//!   id (Wayland's app id, X11's window class); X11 also takes the window's own icon.

use std::sync::{Arc, LazyLock};

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

/// The window's own icon, which only X11 takes (`_NET_WM_ICON`); `None` elsewhere. Made once,
/// for the first window: each one opened after shares it.
pub fn window_icon() -> Option<Arc<image::RgbaImage>> {
    static ICON: LazyLock<Option<Arc<image::RgbaImage>>> = LazyLock::new(|| {
        if !cfg!(any(target_os = "linux", target_os = "freebsd")) {
            return None;
        }
        let icon = image::load_from_memory_with_format(ICON_PNG, image::ImageFormat::Png).ok()?;
        // X11 is sent every pixel, and task bars show it far smaller than 512px.
        let icon = icon.resize(128, 128, image::imageops::FilterType::Lanczos3);
        Some(Arc::new(icon.into_rgba8()))
    });
    ICON.clone()
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

    /// What the entries Gitto writes say they are: one without it is a user's, or an installer's.
    const WRITTEN_BY_GITTO: &str =
        "# Written by Gitto when it runs without being installed, for its name and icon.";

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
        register_in(&data_home, &env::current_exe()?)
    }

    /// Writes the entry of the program `exe`, and its icon, in `data_home`. Not over an entry
    /// Gitto didn't write: one installed in the home folder, or the user's own. Nor over another
    /// build's, as long as its program's still there: builds run from other worktrees, or target
    /// directories, don't take it from each other each time they start.
    fn register_in(data_home: &Path, exe: &Path) -> io::Result<()> {
        let path = data_home
            .join("applications")
            .join(format!("{APP_ID}.desktop"));
        let written = fs::read_to_string(&path).ok();
        if written
            .as_deref()
            .is_some_and(|written| !written.contains(WRITTEN_BY_GITTO))
        {
            return Ok(());
        }
        let icon = data_home
            .join("icons/hicolor/512x512/apps")
            .join(format!("{APP_ID}.png"));
        write_if_changed(&icon, ICON_PNG)?;
        if written
            .as_deref()
            .is_some_and(|written| is_another_builds(written, &icon))
        {
            return Ok(());
        }
        write_if_changed(&path, desktop_entry(exe, &icon).as_bytes())
    }

    /// Whether `written` is the entry another build wrote, with `icon`, whose program is still
    /// there: all but its `Exec` as this build's would be.
    fn is_another_builds(written: &str, icon: &Path) -> bool {
        program_of(written)
            .filter(|program| program.is_file())
            .is_some_and(|program| desktop_entry(&program, icon) == written)
    }

    /// The program an entry's `Exec` runs, as [`exec_argument`] quoted it.
    fn program_of(entry: &str) -> Option<PathBuf> {
        let exec = entry.lines().find_map(|line| line.strip_prefix("Exec="))?;
        let quoted = exec
            .strip_suffix(" %U")?
            .replace("%%", "%")
            .replace("\\\\", "\\");
        let inner = quoted.strip_prefix('"')?.strip_suffix('"')?;
        let mut path = String::new();
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            path.push(if c == '\\' { chars.next()? } else { c });
        }
        Some(PathBuf::from(path))
    }

    fn desktop_entry(exe: &Path, icon: &Path) -> String {
        format!(
            "[Desktop Entry]\n\
             {WRITTEN_BY_GITTO}\n\
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
        fn reads_the_program_back_from_exec() {
            for program in [
                "/opt/gitto/gitto",
                "/home/me/my apps/gitto",
                "/a\"b$c`d\\e%f/gitto",
            ] {
                let entry = desktop_entry(Path::new(program), Path::new("/icons/gitto.png"));
                assert_eq!(program_of(&entry), Some(PathBuf::from(program)));
            }
        }

        #[test]
        fn leaves_another_builds_entry_while_its_program_is_there() {
            let dir = tempfile::tempdir().unwrap();
            let icon = dir.path().join("gitto.png");
            let program = dir.path().join("other/gitto");
            let written = desktop_entry(&program, &icon);
            // Deleted with its target directory, say.
            assert!(!is_another_builds(&written, &icon));
            fs::create_dir_all(program.parent().unwrap()).unwrap();
            fs::write(&program, b"").unwrap();
            assert!(is_another_builds(&written, &icon));
            // Not if it's of another icon, or written by an older Gitto.
            assert!(!is_another_builds(&written, &dir.path().join("old.png")));
            let older = written.replace("NoDisplay=true\n", "");
            assert!(!is_another_builds(&older, &icon));
        }

        #[test]
        fn writes_its_entry_over_its_own_but_not_over_anothers() {
            let dir = tempfile::tempdir().unwrap();
            let exe = dir.path().join("target/gitto");
            let entry = dir.path().join(format!("applications/{APP_ID}.desktop"));
            let icon = dir
                .path()
                .join(format!("icons/hicolor/512x512/apps/{APP_ID}.png"));

            register_in(dir.path(), &exe).unwrap();
            assert_eq!(
                fs::read_to_string(&entry).unwrap(),
                desktop_entry(&exe, &icon)
            );
            assert!(icon.is_file());

            // An older Gitto's: replaced.
            let older = desktop_entry(&exe, &icon).replace("NoDisplay=true\n", "");
            fs::write(&entry, &older).unwrap();
            register_in(dir.path(), &exe).unwrap();
            assert_eq!(
                fs::read_to_string(&entry).unwrap(),
                desktop_entry(&exe, &icon)
            );

            // A user's, or one installed in the home folder: left as it is, without an icon of
            // Gitto's beside it.
            let installed =
                "[Desktop Entry]\nType=Application\nName=Gitto\nExec=/opt/Gitto/gitto %U\n";
            fs::write(&entry, installed).unwrap();
            fs::remove_file(&icon).unwrap();
            register_in(dir.path(), &exe).unwrap();
            assert_eq!(fs::read_to_string(&entry).unwrap(), installed);
            assert!(!icon.exists());
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
            // Made once: another window shares it.
            assert!(Arc::ptr_eq(&icon, &window_icon().unwrap()));
        } else {
            assert!(window_icon().is_none());
        }
    }
}
