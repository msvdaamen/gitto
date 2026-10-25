//! What the sidebar's Agents view shows. Ported from
//! `components/repository/sidebar/agent-placeholder.tsx`.

use gpui_kit::component::button::Button;
use gpui_kit::component::{Sizable as _, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{App, FontWeight, Window, div};

use super::base_px;
use gitto_app_kit::theme::PaletteExt as _;
use gitto_app_kit::ui::Mascot;

/// What the sidebar's Agents view shows until agent sessions exist: the mascot, saying none are
/// running, and a button to start one (which does nothing yet). In the rail (`rail`), the mascot
/// alone.
#[derive(IntoElement)]
pub struct AgentPlaceholder {
    rail: bool,
}

impl AgentPlaceholder {
    pub fn new(rail: bool) -> Self {
        AgentPlaceholder { rail }
    }
}

impl RenderOnce for AgentPlaceholder {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = *cx.palette();
        v_flex()
            .items_center()
            .text_center()
            .when(!self.rail, |this| this.px(base_px(18.)).py(base_px(65.)))
            .when(self.rail, |this| this.py_4())
            .child(Mascot::new(base_px(38.)))
            .when(!self.rail, |this| {
                this.child(
                    div()
                        .mt_3()
                        .text_size(base_px(13.5))
                        .font_weight(FontWeight::BOLD)
                        .child("No agents running"),
                )
                .child(
                    div()
                        .mt_1p5()
                        .mb_3()
                        .text_size(base_px(11.5))
                        .text_color(palette.muted)
                        .child("Agent sessions will appear here alongside their worktrees."),
                )
                .child(
                    Button::new("start-session")
                        .outline()
                        .xsmall()
                        .label("Start a session"),
                )
            })
    }
}
