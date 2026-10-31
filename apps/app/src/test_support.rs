//! For tests: the app's globals on an in-memory database, and a window to render views in.

use gpui_kit::{
    AnyWindowHandle, AppContext as _, Bounds, Context, Entity, Render, TestAppContext,
    WindowBounds, WindowOptions, px, size,
};
use tempfile::TempDir;

use gitto_app_data::services::Services;
use gitto_app_data::state::router::Route;

/// Sets up what the app sets up (see `app::init`), on services `configure` makes of in-memory
/// ones; the folder they keep their files in lives as long as what's returned.
///
/// Motion is reduced: dialogs and popovers then open where they end up at once, rather than after
/// an animation timed by the wall clock, which a test's clicks could land in the middle of. Times
/// are told in English ("3 minutes ago"), whatever the system's language.
pub fn init(cx: &mut TestAppContext, configure: impl FnOnce(Services) -> Services) -> TempDir {
    cx.update(gpui_kit::init);
    cx.update(|cx| cx.set_reduce_motion(true));
    gitto_ui_logic::format::tell_relative_times_in("en");
    let dir = tempfile::tempdir().expect("a temporary folder");
    let services = configure(Services::in_memory(dir.path()).expect("in-memory services"));
    cx.update(|cx| crate::app::init(services, Route::Home, cx));
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

/// Updates that a test says, and counts the installs asked for.
pub struct TestUpdates {
    states: std::sync::Mutex<
        Option<
            futures::channel::mpsc::UnboundedReceiver<
                gitto_app_data::services::system::UpdateState,
            >,
        >,
    >,
    pub installs: std::sync::atomic::AtomicUsize,
}

impl TestUpdates {
    /// The updates, and what sends their states.
    pub fn new() -> (
        std::sync::Arc<TestUpdates>,
        futures::channel::mpsc::UnboundedSender<gitto_app_data::services::system::UpdateState>,
    ) {
        let (sender, receiver) = futures::channel::mpsc::unbounded();
        let updates = TestUpdates {
            states: std::sync::Mutex::new(Some(receiver)),
            installs: std::sync::atomic::AtomicUsize::new(0),
        };
        (std::sync::Arc::new(updates), sender)
    }
}

impl gitto_app_data::services::system::Updates for TestUpdates {
    fn watch(
        &self,
    ) -> futures::stream::BoxStream<'static, gitto_app_data::services::system::UpdateState> {
        use futures::StreamExt as _;
        self.states
            .lock()
            .unwrap()
            .take()
            .expect("watched once")
            .boxed()
    }

    fn install(&self) {
        self.installs
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// `services`, with `updates`.
pub fn with_updates(services: Services, updates: std::sync::Arc<TestUpdates>) -> Services {
    let mut system = services.system().clone();
    system.updates = updates;
    services.with_system(system)
}
