//! **Stub.** Whether Gitto sends error reports. Replaces
//! `components/settings/telemetry-setting.tsx`.

use gpui_kit::prelude::*;
use gpui_kit::{Context, Window, div};

use crate::components::placeholder::Placeholder;
use crate::services::Services;
use crate::services::system::TelemetryState;

/// Whether Gitto sends error reports: a switch, saved through `Services::system().telemetry`
/// (which keeps the user's setting; see its stub). An entity, as it keeps whether it's saving and
/// whether that failed. Stub: renders a placeholder with the state.
pub struct TelemetrySetting {
    state: TelemetryState,
}

impl TelemetrySetting {
    pub fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
        TelemetrySetting {
            state: Services::global(cx).system().telemetry.state(),
        }
    }
}

impl Render for TelemetrySetting {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().h_16().child(
            Placeholder::new(
                "Send error reports",
                "components/settings/telemetry-setting.tsx",
            )
            .detail(format!(
                "available: {}, enabled: {}, sending: {}",
                self.state.available, self.state.enabled, self.state.sending
            )),
        )
    }
}
