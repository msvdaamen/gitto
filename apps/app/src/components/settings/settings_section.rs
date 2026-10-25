//! A group of settings. Ported from `components/settings/settings-section.tsx`.

use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, SharedString, Window, div};

use gitto_app_kit::theme::PaletteExt as _;
use gitto_app_kit::ui::{Icon, IconName};

/// A group of settings on the settings page, like Privacy: its title, what it's about, and its
/// settings (its children), each in a row of its own, with a line between them.
#[derive(IntoElement)]
pub struct SettingsSection {
    icon: IconName,
    title: SharedString,
    description: SharedString,
    children: Vec<AnyElement>,
}

impl SettingsSection {
    pub fn new(
        icon: IconName,
        title: impl Into<SharedString>,
        description: impl Into<SharedString>,
    ) -> Self {
        SettingsSection {
            icon,
            title: title.into(),
            description: description.into(),
            children: Vec::new(),
        }
    }
}

impl ParentElement for SettingsSection {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for SettingsSection {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = *cx.palette();
        let last = self.children.len().saturating_sub(1);
        v_flex()
            .rounded(cx.theme().radius_lg)
            .border_1()
            .border_color(palette.border)
            .bg(palette.panel)
            .p_5()
            .child(
                h_flex()
                    .items_start()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .size_8()
                            .items_center()
                            .justify_center()
                            .rounded(cx.theme().radius)
                            .bg(palette.primary_soft)
                            .text_color(palette.primary_strong)
                            .child(Icon::new(self.icon).size_4()),
                    )
                    .child(
                        v_flex()
                            .min_w_0()
                            .gap_0p5()
                            .child(div().text_base().text_color(palette.text).child(self.title))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(palette.muted)
                                    .child(self.description),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .mt_4()
                    .border_t_1()
                    .border_color(palette.border_soft)
                    .children(self.children.into_iter().enumerate().map(|(ix, child)| {
                        div()
                            .pt_4()
                            .when(ix < last, |this| {
                                this.pb_4().border_b_1().border_color(palette.border_soft)
                            })
                            .child(child)
                    })),
            )
    }
}
