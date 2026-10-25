//! For tests: GPUI Kit and the theme on in-memory services, and a window to render views in.

use gitto_app_data::services::Services;
use gpui_kit::{
    AnyWindowHandle, AppContext as _, Bounds, Context, Entity, Render, TestAppContext,
    WindowBounds, WindowOptions, px, size,
};
use tempfile::TempDir;

/// Sets up GPUI Kit, and the services and theme the primitives read, as the app does; the
/// folder the services keep their files in lives as long as what's returned. Motion is reduced,
/// so dialogs open where they end up at once.
pub fn init(cx: &mut TestAppContext) -> TempDir {
    cx.update(gpui_kit::init);
    cx.update(|cx| cx.set_reduce_motion(true));
    let dir = tempfile::tempdir().expect("a temporary folder");
    let services = Services::in_memory(dir.path()).expect("in-memory services");
    let storage = services.storage().clone();
    cx.update(|cx| {
        cx.set_global(services);
        crate::theme::init(Some(storage), cx);
    });
    dir
}

/// Opens a 1200 by 800 window showing the view `build` makes.
pub fn open<V: Render>(
    cx: &mut TestAppContext,
    build: impl FnOnce(&mut gpui_kit::Window, &mut Context<V>) -> V,
) -> (AnyWindowHandle, Entity<V>) {
    let (window, view) = cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Default::default(),
                    size(px(1200.), px(800.)),
                ))),
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| build(window, cx)),
        )
        .expect("open a test window")
    });
    cx.run_until_parked();
    (window, view)
}
