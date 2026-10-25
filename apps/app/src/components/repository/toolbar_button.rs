//! **Stub.** One of the repository toolbar's actions. Replaces
//! `components/repository/toolbar-button.tsx`.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Disableable as _, Sizable as _};
use gpui_kit::prelude::*;
use gpui_kit::{App, ClickEvent, ElementId, SharedString, Window};

use crate::components::ui::{Icon, IconName};

/// Below which window width a toolbar button hides its label (TS's `hideBelow`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HideBelow {
    /// 700px.
    Small,
    /// 1100px.
    Large,
}

type OnClick = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// One of the repository toolbar's actions: an icon over its label. Stub: a ghost button with
/// its icon and label.
#[derive(IntoElement)]
pub struct ToolbarButton {
    id: ElementId,
    icon: IconName,
    label: SharedString,
    hide_below: Option<HideBelow>,
    disabled: bool,
    loading: bool,
    on_click: Option<OnClick>,
}

impl ToolbarButton {
    pub fn new(id: impl Into<ElementId>, icon: IconName, label: impl Into<SharedString>) -> Self {
        ToolbarButton {
            id: id.into(),
            icon,
            label: label.into(),
            hide_below: None,
            disabled: false,
            loading: false,
            on_click: None,
        }
    }

    pub fn hide_below(mut self, width: HideBelow) -> Self {
        self.hide_below = Some(width);
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// It's running: shown so, and not clickable meanwhile.
    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
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

impl RenderOnce for ToolbarButton {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let _ = self.hide_below;
        Button::new(self.id)
            .ghost()
            .small()
            .icon(Icon::new(self.icon))
            .label(self.label)
            .disabled(self.disabled)
            .loading(self.loading)
            .when_some(self.on_click, |button, on_click| button.on_click(on_click))
    }
}
