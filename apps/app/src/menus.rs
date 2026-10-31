//! The application menu, and the keys of what's in it. The Electron app had Electron's default
//! menu (main.ts sets none of its own): on macOS, the app menu, Edit, View and Window. GPUI has
//! none, so they're Gitto's own here, on macOS, where the menu bar is the app's; zooming and full
//! screen have their keys everywhere, as Electron's View menu gave them.

use gpui_kit::component::input::{Copy, Cut, Paste, Redo, SelectAll, Undo};
use gpui_kit::{App, KeyBinding, Menu, MenuItem, OsAction, actions};

use crate::app::{Quit, come_forward};
use gitto_app_data::state::router::{Route, Router};
use gitto_app_data::state::whats_new::WhatsNew;
use gitto_app_kit::theme::zoom;

actions!(
    gitto,
    [
        /// Shows which Gitto this is, and what's new in it.
        About,
        /// Opens the settings.
        Settings,
        /// Hides Gitto.
        Hide,
        /// Hides the other apps.
        HideOthers,
        /// Shows the apps hidden.
        ShowAll,
        /// Draws the UI a step bigger.
        ZoomIn,
        /// Draws the UI a step smaller.
        ZoomOut,
        /// Draws the UI at its size.
        ResetZoom,
        /// Puts the window in full screen, or takes it out.
        ToggleFullScreen,
        /// Minimizes the window.
        Minimize,
        /// Zooms the window (macOS's), filling the screen, or back.
        ZoomWindow,
        /// Closes the window.
        CloseWindow,
    ]
);

/// Binds the menu's keys, handles its commands, and sets the menu bar on macOS. Call once, as the
/// app starts, after `app::init`'s other key bindings.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("secondary-=", ZoomIn, None),
        KeyBinding::new("secondary-+", ZoomIn, None),
        KeyBinding::new("secondary--", ZoomOut, None),
        KeyBinding::new("secondary-0", ResetZoom, None),
    ]);
    if cfg!(target_os = "macos") {
        cx.bind_keys([
            KeyBinding::new("cmd-,", Settings, None),
            KeyBinding::new("cmd-h", Hide, None),
            KeyBinding::new("alt-cmd-h", HideOthers, None),
            KeyBinding::new("ctrl-cmd-f", ToggleFullScreen, None),
            KeyBinding::new("cmd-m", Minimize, None),
            KeyBinding::new("cmd-w", CloseWindow, None),
        ]);
    } else {
        cx.bind_keys([KeyBinding::new("f11", ToggleFullScreen, None)]);
    }

    cx.on_action(|_: &About, cx| {
        come_forward(cx);
        WhatsNew::global(cx).update(cx, |whats_new, cx| whats_new.show_all(cx));
    });
    cx.on_action(|_: &Settings, cx| {
        come_forward(cx);
        Router::navigate(Route::Settings, cx);
    });
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.on_action(|_: &ZoomIn, cx| zoom::zoom_in(cx));
    cx.on_action(|_: &ZoomOut, cx| zoom::zoom_out(cx));
    cx.on_action(|_: &ResetZoom, cx| zoom::reset(cx));
    cx.on_action(|_: &ToggleFullScreen, cx| {
        on_active_window(cx, |window| window.toggle_fullscreen())
    });
    cx.on_action(|_: &Minimize, cx| on_active_window(cx, |window| window.minimize_window()));
    cx.on_action(|_: &ZoomWindow, cx| on_active_window(cx, |window| window.zoom_window()));
    cx.on_action(|_: &CloseWindow, cx| on_active_window(cx, |window| window.remove_window()));

    // After the keys, which the menu bar shows beside its items as it's set.
    if cfg!(target_os = "macos") {
        cx.set_menus(menus());
    }
}

/// The menu bar: the app menu, Edit, View and Window, as Electron's default one on macOS. Edit's
/// items are the inputs' (they do nothing elsewhere), and the OS's own on macOS.
pub fn menus() -> Vec<Menu> {
    vec![
        Menu::new("Gitto").items([
            MenuItem::action("About Gitto", About),
            MenuItem::separator(),
            MenuItem::action("Settings…", Settings),
            MenuItem::separator(),
            MenuItem::action("Hide Gitto", Hide),
            MenuItem::action("Hide Others", HideOthers),
            MenuItem::action("Show All", ShowAll),
            MenuItem::separator(),
            MenuItem::action("Quit Gitto", Quit),
        ]),
        Menu::new("Edit").items([
            MenuItem::os_action("Undo", Undo, OsAction::Undo),
            MenuItem::os_action("Redo", Redo, OsAction::Redo),
            MenuItem::separator(),
            MenuItem::os_action("Cut", Cut, OsAction::Cut),
            MenuItem::os_action("Copy", Copy, OsAction::Copy),
            MenuItem::os_action("Paste", Paste, OsAction::Paste),
            MenuItem::os_action("Select All", SelectAll, OsAction::SelectAll),
        ]),
        Menu::new("View").items([
            MenuItem::action("Actual Size", ResetZoom),
            MenuItem::action("Zoom In", ZoomIn),
            MenuItem::action("Zoom Out", ZoomOut),
            MenuItem::separator(),
            MenuItem::action("Toggle Full Screen", ToggleFullScreen),
        ]),
        Menu::new("Window").items([
            MenuItem::action("Minimize", Minimize),
            MenuItem::action("Zoom", ZoomWindow),
            MenuItem::separator(),
            MenuItem::action("Close Window", CloseWindow),
        ]),
    ]
}

/// Runs `run` on the window that's active, if one is.
fn on_active_window(cx: &mut App, run: impl FnOnce(&mut gpui_kit::Window)) {
    if let Some(window) = cx.active_window() {
        let _ = window.update(cx, |_, window, _| run(window));
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{AppContext as _, Context, IntoElement, Render, TestAppContext, Window, div};

    use super::*;
    use crate::test_support;

    struct Host;

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    #[gpui_kit::test]
    fn zooms_with_the_keys(cx: &mut TestAppContext) {
        let _dir = test_support::init(cx, |services| services);
        let (window, _) = test_support::open(cx, |_, _| Host);
        let press = |key: &str, cx: &mut TestAppContext| {
            cx.update_window(window, |_, window, cx| window.press(key, cx))
                .unwrap();
            cx.update(|cx| zoom::zoom(cx))
        };
        assert_eq!(press("secondary-=", cx), 1.1);
        assert_eq!(press("secondary-=", cx), 1.25);
        assert_eq!(press("secondary--", cx), 1.1);
        assert_eq!(press("secondary-0", cx), 1.);
    }

    #[gpui_kit::test]
    fn opens_the_settings(cx: &mut TestAppContext) {
        let _dir = test_support::init(cx, |services| services);
        test_support::open(cx, |_, _| Host);
        cx.update(|cx| cx.dispatch_action(&Settings));
        assert_eq!(cx.update(|cx| Router::current(cx)), Route::Settings);
    }

    #[test]
    fn has_the_menus_electrons_default_menu_had_on_macos() {
        let names: Vec<String> = menus().iter().map(|menu| menu.name.to_string()).collect();
        assert_eq!(names, ["Gitto", "Edit", "View", "Window"]);
    }
}
