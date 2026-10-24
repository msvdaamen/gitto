//! Gitto's settings. Ported from `routes/(main)/settings.tsx`.

use gpui_kit::component::v_flex;
use gpui_kit::prelude::*;
use gpui_kit::{Context, Entity, Window, div, rems};

use crate::components::settings::settings_section::SettingsSection;
use crate::components::settings::telemetry_setting::TelemetrySetting;
use crate::components::ui::IconName;
use crate::theme::PaletteExt as _;

/// The widest the settings get (`max-w-180`, 720px), in rems.
pub const CONTENT_MAX_WIDTH: f32 = 48.;

/// Gitto's settings, a section for each kind.
pub struct SettingsView {
    telemetry: Entity<TelemetrySetting>,
}

impl SettingsView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        SettingsView {
            telemetry: cx.new(|cx| TelemetrySetting::new(window, cx)),
        }
    }
}

impl Render for SettingsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = *cx.palette();
        div()
            .id("settings")
            .size_full()
            .overflow_y_scroll()
            .bg(palette.bg)
            .px_10()
            .pt_6()
            .pb_10()
            .child(
                v_flex()
                    .mx_auto()
                    .max_w(rems(CONTENT_MAX_WIDTH))
                    .w_full()
                    .gap_4()
                    .child(
                        v_flex()
                            .child(div().text_2xl().text_color(palette.text).child("Settings"))
                            .child(
                                div()
                                    .mt_1()
                                    .text_sm()
                                    .text_color(palette.muted)
                                    .child("They apply to Gitto on this computer."),
                            ),
                    )
                    .child(
                        SettingsSection::new(
                            IconName::ShieldCheck,
                            "Privacy",
                            "What Gitto sends, and what it keeps to itself.",
                        )
                        .child(self.telemetry.clone()),
                    ),
            )
    }
}
