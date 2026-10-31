//! Whether Gitto sends error reports. Ported from `components/settings/telemetry-setting.tsx`.

use gpui_kit::prelude::*;
use gpui_kit::{Context, Role, Task, TestSupportExt as _, Window, div};

use crate::components::ui::SettingSwitch;
use crate::services::Services;
use crate::services::system::TelemetryState;
use crate::theme::PaletteExt as _;

/// Where saving the setting is.
enum Save {
    Idle,
    /// Saving it; dropping the task stops waiting for it.
    Saving {
        _task: Task<()>,
    },
    /// The last save failed.
    Failed,
}

/// Whether Gitto sends error reports: a switch, saved through `Services::system().telemetry`
/// (which keeps the user's setting; see its stub). An entity, as it keeps whether it's saving and
/// whether that failed. The switch stays as it is, focus and all, when the state changes.
pub struct TelemetrySetting {
    state: TelemetryState,
    save: Save,
}

impl TelemetrySetting {
    pub fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
        TelemetrySetting {
            state: Services::global(cx).system().telemetry.state(),
            save: Save::Idle,
        }
    }

    /// Lets Gitto send error reports, or not; says if that couldn't be saved, and keeps the switch
    /// as it was then.
    pub fn set_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        let saving = Services::global(cx).system().telemetry.set_enabled(enabled);
        self.save = Save::Saving {
            _task: cx.spawn(async move |this, cx| {
                let result = saving.await;
                let _ = this.update(cx, |this, cx| {
                    this.save = match result {
                        Ok(state) => {
                            this.state = state;
                            Save::Idle
                        }
                        Err(error) => {
                            log::error!(
                                "Couldn't change whether Gitto sends error reports: {error}"
                            );
                            Save::Failed
                        }
                    };
                    cx.notify();
                });
            }),
        };
        cx.notify();
    }
}

impl Render for TelemetrySetting {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = *cx.palette();
        let current = self.state;
        SettingSwitch::new("send-error-reports", "Send error reports")
            .checked(current.enabled)
            .disabled(!current.available || matches!(self.save, Save::Saving { .. }))
            .on_change(cx.listener(|this, enabled: &bool, _, cx| this.set_enabled(*enabled, cx)))
            .child(div().child(
                "When something goes wrong, Gitto tells its developers what and where. Reports \
                 can name files and branches, and include Git's errors, but not what's in your \
                 files. Paths start from your home folder, ~, and passwords in URLs are left out.",
            ))
            .when(!current.available, |this| {
                this.child(
                    div()
                        .id("telemetry-unavailable")
                        .test_support()
                        .text_color(palette.faint)
                        .child("This build of Gitto doesn't send error reports."),
                )
            })
            .when(
                current.available && current.enabled && !current.sending,
                |this| {
                    this.child(
                        div()
                            .id("telemetry-on-restart")
                            .test_support()
                            .text_color(palette.faint)
                            .child("Gitto starts sending them when it restarts."),
                    )
                },
            )
            .when(matches!(self.save, Save::Failed), |this| {
                this.child(
                    div()
                        .id("telemetry-failed")
                        .test_support()
                        .role(Role::Status)
                        .aria_label("Couldn't save the setting. Try again.")
                        .text_color(palette.coral)
                        .child("Couldn't save the setting. Try again."),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::{Arc, Mutex};

    use futures::FutureExt as _;
    use futures::future::BoxFuture;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{AnyWindowHandle, Entity, TestAppContext};

    use super::*;
    use crate::services::system::Telemetry;
    use crate::test_support;

    /// The setting as a test says it is, with what saving it gives, and what was saved.
    struct TestTelemetry {
        state: TelemetryState,
        saved: Mutex<Result<TelemetryState, String>>,
        calls: Mutex<Vec<bool>>,
    }

    impl Telemetry for TestTelemetry {
        fn state(&self) -> TelemetryState {
            self.state
        }

        fn set_enabled(&self, enabled: bool) -> BoxFuture<'static, io::Result<TelemetryState>> {
            self.calls.lock().unwrap().push(enabled);
            let result = self.saved.lock().unwrap().clone().map_err(io::Error::other);
            async move { result }.boxed()
        }
    }

    fn state(available: bool, enabled: bool, sending: bool) -> TelemetryState {
        TelemetryState {
            available,
            enabled,
            sending,
        }
    }

    /// The setting, in `current` state, saving to `saved`.
    fn render_with(
        current: TelemetryState,
        saved: Result<TelemetryState, String>,
        cx: &mut TestAppContext,
    ) -> (
        AnyWindowHandle,
        Entity<TelemetrySetting>,
        Arc<TestTelemetry>,
        tempfile::TempDir,
    ) {
        let telemetry = Arc::new(TestTelemetry {
            state: current,
            saved: Mutex::new(saved),
            calls: Mutex::default(),
        });
        let dir = test_support::init(cx, |services| {
            let mut system = services.system().clone();
            system.telemetry = telemetry.clone();
            services.with_system(system)
        });
        let (window, setting) = test_support::open(cx, TelemetrySetting::new);
        cx.update_window(window, |_, window, cx| window.render_frame(cx))
            .unwrap();
        (window, setting, telemetry, dir)
    }

    #[gpui_kit::test]
    fn turns_error_reports_off(cx: &mut TestAppContext) {
        let (window, _setting, telemetry, _dir) =
            render_with(state(true, true, true), Ok(state(true, false, false)), cx);
        cx.update_window(window, |_, window, cx| {
            assert_eq!(window.find("send-error-reports").checked(), Some(true));
            window.click("send-error-reports", cx);
        })
        .unwrap();
        cx.run_until_parked();
        assert_eq!(*telemetry.calls.lock().unwrap(), [false]);
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(window.find("send-error-reports").checked(), Some(false));
            assert!(window.try_find("telemetry-on-restart").is_none());
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn says_turning_them_on_starts_them_when_gitto_restarts(cx: &mut TestAppContext) {
        let (window, _setting, telemetry, _dir) =
            render_with(state(true, false, false), Ok(state(true, true, false)), cx);
        cx.update_window(window, |_, window, cx| {
            window.click("send-error-reports", cx)
        })
        .unwrap();
        cx.run_until_parked();
        assert_eq!(*telemetry.calls.lock().unwrap(), [true]);
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("telemetry-on-restart").is_some());
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn cant_be_changed_in_a_build_that_doesnt_send_them(cx: &mut TestAppContext) {
        let (window, _setting, telemetry, _dir) = render_with(
            state(false, true, false),
            Ok(state(false, false, false)),
            cx,
        );
        cx.update_window(window, |_, window, cx| {
            assert_eq!(window.find("send-error-reports").disabled(), Some(true));
            assert!(window.try_find("telemetry-unavailable").is_some());
            window.click("send-error-reports", cx);
        })
        .unwrap();
        cx.run_until_parked();
        assert!(telemetry.calls.lock().unwrap().is_empty());
    }

    #[gpui_kit::test]
    fn says_when_the_setting_couldnt_be_saved(cx: &mut TestAppContext) {
        let (window, _setting, _telemetry, _dir) =
            render_with(state(true, true, true), Err("disk full".into()), cx);
        cx.update_window(window, |_, window, cx| {
            window.click("send-error-reports", cx)
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(
                window.find("telemetry-failed").label(),
                Some("Couldn't save the setting. Try again.")
            );
            assert_eq!(window.find("send-error-reports").checked(), Some(true));
        })
        .unwrap();
    }
}
