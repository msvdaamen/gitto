//! Starting the app: the services, the theme, the data layer, and the window. Ported from
//! `index.tsx` (the renderer's start) and `apps/electron/src/main.ts` (the window, and what's
//! started before it).

use gpui_kit::prelude::*;
use gpui_kit::{
    App, Bounds, KeyBinding, TitlebarOptions, WindowBounds, WindowOptions, actions, px, size,
};

use crate::assets::AppAssets;
use crate::services::{AppPaths, Services, logger};
use crate::state::router::{Route, Router};
use crate::views::root::AppShell;

actions!(
    gitto,
    [
        /// Quits Gitto.
        Quit
    ]
);

/// The window's size when it opens (`createWindow` in main.ts), in pixels: the platform's window
/// geometry, outside the app's rem scale.
pub const WINDOW_WIDTH: f32 = 1200.;
pub const WINDOW_HEIGHT: f32 = 800.;
pub const MIN_WIDTH: f32 = 560.;
pub const MIN_HEIGHT: f32 = 400.;

/// Runs Gitto: starts its services, then opens its window. Quits once the window's closed, as
/// the Electron app did everywhere but macOS (`window-all-closed`); there, it stays running, and
/// opens the window again when it's launched again, from the Dock say (`activate`).
pub fn run() {
    logger::init();
    let application = gpui_kit::application().with_assets(AppAssets);
    application.on_reopen(|cx| {
        if cx.windows().is_empty() && cx.has_global::<Services>() {
            open_main_window(cx);
        }
    });
    application.run(|cx: &mut App| {
        gpui_kit::init(cx);
        let services = match start_services() {
            Ok(services) => services,
            Err(error) => {
                log::error!("Gitto couldn't start: {error:#}");
                cx.quit();
                return;
            }
        };
        init(services, Route::Home, cx);
        open_main_window(cx);
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() && !cfg!(target_os = "macos") {
                cx.quit();
            }
        })
        .detach();
    });
}

/// The services, on the database in Gitto's `userData` folder (see `AppPaths::resolve`).
fn start_services() -> anyhow::Result<Services> {
    let paths = AppPaths::resolve()
        .ok_or_else(|| anyhow::anyhow!("There's no home folder to keep Gitto's files in."))?;
    Services::start(paths)
}

/// Sets up what every window needs, on `services`, starting on `route`: the services, the theme
/// the user picked, the data layer, the router and the key bindings. Call after `gpui_kit::init`.
pub fn init(services: Services, route: Route, cx: &mut App) {
    let storage = services.storage().clone();
    cx.set_global(services);
    crate::theme::init(Some(storage), cx);
    crate::data::init(cx);
    Router::init(route, cx);
    cx.bind_keys([KeyBinding::new("secondary-q", Quit, None)]);
    crate::components::repository::sidebar::ref_list::bind_keys(cx);
    crate::components::repository::history::history_table::bind_keys(cx);
    cx.on_action(|_: &Quit, cx| cx.quit());
}

/// Opens Gitto's window, centered.
pub fn open_main_window(cx: &mut App) {
    let bounds = Bounds::centered(None, size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some("Gitto".into()),
            ..TitlebarOptions::default()
        }),
        app_id: Some("gitto".to_owned()),
        // Where the history still has room beside the sidebar's rail.
        window_min_size: Some(size(px(MIN_WIDTH), px(MIN_HEIGHT))),
        ..WindowOptions::default()
    };
    if let Err(error) = gpui_kit::open_window(options, cx, |window, cx| {
        cx.new(|cx| AppShell::new(window, cx))
    }) {
        log::error!("Gitto couldn't open its window: {error:#}");
        cx.quit();
    }
}
