//! Starting the app: the services, the theme, the data layer, and the window. Ported from
//! `index.tsx` (the renderer's start) and `apps/electron/src/main.ts` (the window, and what's
//! started before it).

use gpui_kit::prelude::*;
use gpui_kit::{
    App, Bounds, Context, KeyBinding, PromptLevel, TitlebarOptions, Window, WindowBounds,
    WindowOptions, actions, div, px, size,
};

use crate::assets::AppAssets;
use crate::identity;
use crate::single_instance::{self, Instance, Running};
use crate::views::root::AppShell;
use gitto_app_data::services::{AppPaths, Services, logger};
use gitto_app_data::state::router::{Route, Router};
use gitto_app_kit::theme;

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
/// opens the window again when it's launched again, from the Dock say (`activate`). Launched
/// again while it runs, on the same profile, the one running comes forward instead (see
/// `single_instance`).
pub fn run() {
    logger::init();
    let paths = AppPaths::resolve();
    let running = match paths
        .as_ref()
        .map(|paths| single_instance::claim(paths.user_data()))
    {
        Some(Ok(Instance::First(running))) => Some(running),
        Some(Ok(Instance::Second)) => return,
        Some(Err(error)) => {
            log::warn!("Couldn't tell whether Gitto's running already: {error}");
            None
        }
        None => None,
    };
    // Git runs with the PATH the user's shell has, as from a terminal, not only the one a Dock or
    // a launcher gave Gitto.
    gitto_git::use_path_from_shell();
    let application = gpui_kit::application().with_assets(AppAssets);
    application.on_reopen(|cx| {
        if cx.windows().is_empty() && cx.has_global::<Services>() {
            open_main_window(cx);
        }
    });
    application.run(|cx: &mut App| {
        gpui_kit::init(cx);
        identity::init(cx);
        let services = match start_services(paths) {
            Ok(services) => services,
            Err(error) => {
                quit_with_error("Gitto couldn't start", &error, cx);
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
        if let Some(running) = running {
            come_forward_when_launched(running, cx);
        }
    });
}

/// The services, on the database in Gitto's `userData` folder (see `AppPaths::resolve`).
fn start_services(paths: Option<AppPaths>) -> anyhow::Result<Services> {
    let paths =
        paths.ok_or_else(|| anyhow::anyhow!("There's no home folder to keep Gitto's files in."))?;
    Services::start(paths)
}

/// Brings Gitto forward each time it's launched again, as `second-instance` did in main.ts.
fn come_forward_when_launched(mut running: Running, cx: &mut App) {
    cx.spawn(async move |cx| {
        while running.launched().await.is_some() {
            cx.update(come_forward);
        }
    })
    .detach();
}

/// Brings Gitto's window forward, or opens it if there's none (on macOS, where Gitto runs on
/// without one).
pub fn come_forward(cx: &mut App) {
    match cx.windows().first() {
        Some(window) => {
            let _ = window.update(cx, |_, window, _| window.activate_window());
        }
        None if cx.has_global::<Services>() => open_main_window(cx),
        None => {}
    }
    cx.activate(true);
}

/// Sets up what every window needs, on `services`, starting on `route`: the services, the theme
/// and zoom the user picked, the data layer, the app's state, the router, the key bindings and
/// the menu bar. Call after `gpui_kit::init`.
pub fn init(services: Services, route: Route, cx: &mut App) {
    let storage = services.storage().clone();
    cx.set_global(services);
    let saved = storage.get_item(theme::STORAGE_KEY);
    let save: theme::SaveTheme = Box::new({
        let storage = storage.clone();
        move |picked| storage.set_item(theme::STORAGE_KEY, picked.as_str())
    });
    theme::init(saved.as_deref(), Some(save), cx);
    let saved_zoom = storage.get_item(theme::zoom::STORAGE_KEY);
    let save_zoom: theme::zoom::SaveZoom = Box::new({
        let storage = storage.clone();
        move |zoom| storage.set_item(theme::zoom::STORAGE_KEY, zoom)
    });
    theme::zoom::init(saved_zoom.as_deref(), Some(save_zoom), cx);
    gitto_app_data::data::init(cx);
    gitto_app_data::state::init(cx);
    Router::init(route, cx);
    cx.bind_keys([KeyBinding::new("secondary-q", Quit, None)]);
    cx.bind_keys(crate::components::repository::diff::viewer::viewer_actions::key_bindings());
    crate::components::repository::sidebar::ref_list::bind_keys(cx);
    crate::components::repository::history::history_table::bind_keys(cx);
    crate::components::tabs::bind_keys(cx);
    gitto_app_kit::ui::resize_handle::bind_keys(cx);
    cx.on_action(|_: &Quit, cx| cx.quit());
    crate::menus::init(cx);
    // What the UI remembers is saved off the UI thread: what's still being saved is, before
    // Gitto goes.
    cx.on_app_quit(move |_| {
        if let Err(error) = storage.flush() {
            log::error!("Couldn't save what the UI remembers: {error}");
        }
        async {}
    })
    .detach();
}

/// Opens Gitto's window, centered.
pub fn open_main_window(cx: &mut App) {
    let bounds = Bounds::centered(None, size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some(identity::APP_NAME.into()),
            ..TitlebarOptions::default()
        }),
        app_id: Some(identity::APP_ID.to_owned()),
        icon: identity::window_icon(),
        // Where the history still has room beside the sidebar's rail.
        window_min_size: Some(size(px(MIN_WIDTH), px(MIN_HEIGHT))),
        ..WindowOptions::default()
    };
    if let Err(error) = gpui_kit::open_window(options, cx, |window, cx| {
        cx.new(|cx| AppShell::new(window, cx))
    }) {
        quit_with_error("Gitto couldn't open its window", &error, cx);
    }
}

/// Says that Gitto can't go on, and why, then quits once that's been read: launched from the
/// Finder, the Dock or the Start menu, it would otherwise just not appear. Said in a window of
/// its own, as there's none of Gitto's to say it over; if not even that opens, it quits at once,
/// with the reason in the log.
pub fn quit_with_error(title: &str, error: &anyhow::Error, cx: &mut App) {
    log::error!("{title}: {error:#}");
    let detail = format!("{error:#}");
    let bounds = Bounds::centered(None, size(px(480.), px(200.)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some(identity::APP_NAME.into()),
            ..TitlebarOptions::default()
        }),
        app_id: Some(identity::APP_ID.to_owned()),
        ..WindowOptions::default()
    };
    let title = title.to_owned();
    let opened = gpui_kit::open_window(options, cx, |_, cx| cx.new(|_| ErrorWindow)).and_then(
        |(window, _)| {
            window.update(cx, |_, window, cx| {
                window.prompt(PromptLevel::Critical, &title, Some(&detail), &["Quit"], cx)
            })
        },
    );
    let Ok(answered) = opened else {
        cx.quit();
        return;
    };
    cx.spawn(async move |cx| {
        let _ = answered.await;
        cx.update(|cx| {
            for window in cx.windows() {
                let _ = window.update(cx, |_, window, _| window.remove_window());
            }
            cx.quit();
        });
    })
    .detach();
}

/// The window Gitto says it can't go on in: empty, under what it says.
struct ErrorWindow;

impl Render for ErrorWindow {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full()
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::TestAppContext;

    use super::*;

    #[gpui_kit::test]
    fn says_why_gitto_cant_go_on_before_it_quits(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        cx.update(|cx| {
            let error = anyhow::anyhow!("The database is locked.");
            quit_with_error("Gitto couldn't start", &error, cx);
        });
        cx.run_until_parked();
        assert!(cx.has_pending_prompt());

        cx.simulate_prompt_answer("Quit");
        cx.run_until_parked();
        assert!(cx.update(|cx| cx.windows().is_empty()));
    }

    #[gpui_kit::test]
    fn comes_forward_in_the_window_it_has(cx: &mut TestAppContext) {
        let _dir = crate::test_support::init(cx, |services| services);
        crate::test_support::open(cx, |_, _| ErrorWindow);
        cx.update(come_forward);
        cx.run_until_parked();
        // Rather than opening another.
        assert_eq!(cx.update(|cx| cx.windows().len()), 1);
    }
}
