//! A choice of a few options, side by side. Ported from `components/ui/segmented-control.tsx`.

use gpui_kit::component::Sizable as _;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::prelude::*;
use gpui_kit::{App, ElementId, SharedString, Window};

type OnChange = Box<dyn Fn(&SharedString, &mut Window, &mut App) + 'static>;

/// A choice of a few options, side by side, the chosen one raised: GPUI Kit's segmented
/// `TabBar`, which keeps the silhouette the Design Guides ask for. Controlled: `on_change` asks
/// for the option picked, and the owner sets `value`.
#[derive(IntoElement)]
pub struct SegmentedControl {
    id: ElementId,
    value: SharedString,
    options: Vec<SharedString>,
    on_change: Option<OnChange>,
}

impl SegmentedControl {
    pub fn new(
        id: impl Into<ElementId>,
        value: impl Into<SharedString>,
        options: impl IntoIterator<Item = impl Into<SharedString>>,
    ) -> Self {
        SegmentedControl {
            id: id.into(),
            value: value.into(),
            options: options.into_iter().map(Into::into).collect(),
            on_change: None,
        }
    }

    pub fn on_change(
        mut self,
        handler: impl Fn(&SharedString, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for SegmentedControl {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let selected = self
            .options
            .iter()
            .position(|option| *option == self.value)
            .unwrap_or(0);
        let options = self.options.clone();
        TabBar::new(self.id)
            .segmented()
            .small()
            .selected_index(selected)
            .children(
                self.options
                    .into_iter()
                    .map(|option| Tab::new().label(option)),
            )
            .when_some(self.on_change, |bar, on_change| {
                bar.on_click(move |ix, window, cx| {
                    if let Some(option) = options.get(*ix) {
                        on_change(option, window, cx);
                    }
                })
            })
    }
}
