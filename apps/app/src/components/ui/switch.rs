//! An on/off setting. Ported from `components/ui/switch.tsx`.

use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Disableable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, ElementId, FontWeight, SharedString, Window, div};

use crate::theme::PaletteExt as _;

type OnChange = Box<dyn Fn(&bool, &mut Window, &mut App) + 'static>;

/// An on/off setting that takes effect at once: its label, and what it does underneath (its
/// children), with GPUI Kit's `Switch` on the right. Controlled: `on_change` asks for the new value,
/// and the owner sets `checked`.
#[derive(IntoElement)]
pub struct SettingSwitch {
    id: ElementId,
    label: SharedString,
    checked: bool,
    disabled: bool,
    on_change: Option<OnChange>,
    children: Vec<AnyElement>,
}

impl SettingSwitch {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        SettingSwitch {
            id: id.into(),
            label: label.into(),
            checked: false,
            disabled: false,
            on_change: None,
            children: Vec::new(),
        }
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_change(mut self, handler: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Box::new(handler));
        self
    }
}

impl ParentElement for SettingSwitch {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for SettingSwitch {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = *cx.palette();
        h_flex()
            .items_start()
            .gap_4()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette.text)
                            .child(self.label.clone()),
                    )
                    .child(
                        v_flex()
                            .mt_1()
                            .gap_1p5()
                            .text_sm()
                            .text_color(palette.text_soft)
                            .children(self.children),
                    ),
            )
            .child(
                Switch::new(self.id)
                    .accessibility_label(self.label)
                    .checked(self.checked)
                    .disabled(self.disabled)
                    .when_some(self.on_change, |switch, on_change| {
                        switch.on_change(on_change)
                    }),
            )
    }
}
