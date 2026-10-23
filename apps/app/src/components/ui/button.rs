//! Buttons. Ported from `components/ui/button.tsx`.
//!
//! `Button` is GPUI Kit's, with Gitto's look from the theme: `.primary()` is the gradient one
//! (the decision area's default commit), the default is TS's `secondary`, `.ghost()` its `ghost`.
//! The two TS shapes Kit has no name for are here: [`IconButton`] and [`LinkButton`].

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Disableable as _, Selectable as _, Sizable as _};
use gpui_kit::prelude::*;
use gpui_kit::{App, ClickEvent, ElementId, SharedString, Window};

use super::icons::{Icon, IconName};

type OnClick = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// A square, icon-only button, named by its `label`, which its tooltip says too. `active` shows
/// it pressed, e.g. while the panel it toggles is open.
#[derive(IntoElement)]
pub struct IconButton {
    id: ElementId,
    icon: IconName,
    label: SharedString,
    active: bool,
    disabled: bool,
    on_click: Option<OnClick>,
}

impl IconButton {
    pub fn new(id: impl Into<ElementId>, icon: IconName, label: impl Into<SharedString>) -> Self {
        IconButton {
            id: id.into(),
            icon,
            label: label.into(),
            active: false,
            disabled: false,
            on_click: None,
        }
    }

    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for IconButton {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        Button::new(self.id)
            .ghost()
            .small()
            .icon(Icon::new(self.icon))
            .accessibility_label(self.label.clone())
            .tooltip(self.label)
            .selected(self.active)
            .disabled(self.disabled)
            .when_some(self.on_click, |button, on_click| button.on_click(on_click))
    }
}

/// A borderless, text-only button, for small actions like "View all", in the primary color.
#[derive(IntoElement)]
pub struct LinkButton {
    id: ElementId,
    label: SharedString,
    icon: Option<IconName>,
    disabled: bool,
    on_click: Option<OnClick>,
}

impl LinkButton {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        LinkButton {
            id: id.into(),
            label: label.into(),
            icon: None,
            disabled: false,
            on_click: None,
        }
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for LinkButton {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        use crate::theme::PaletteExt as _;
        Button::new(self.id)
            .text()
            .small()
            .label(self.label)
            .text_color(cx.palette().primary_strong)
            // The color given above outweighs the button's own for disabled: dimmed instead.
            .when(self.disabled, |button| button.opacity(0.5))
            .when_some(self.icon, |button, icon| button.icon(Icon::new(icon)))
            .disabled(self.disabled)
            .when_some(self.on_click, |button, on_click| button.on_click(on_click))
    }
}
