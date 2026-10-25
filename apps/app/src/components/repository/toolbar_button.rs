//! One of the repository toolbar's actions. Ported from
//! `components/repository/toolbar-button.tsx`.

use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{Disableable as _, Selectable, Sizable as _, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{App, ClickEvent, ElementId, FontWeight, SharedString, Window, div, rems};

use crate::components::ui::{FailurePopover, Icon, IconName};
use crate::data::DataError;
use crate::theme::PaletteExt as _;
use crate::views::repository::{LARGE, MEDIUM};

/// The window width below which the toolbar's gaps tighten and the buttons hidden below `Small`
/// go (`index.css`'s `sm` breakpoint).
pub const SMALL: f32 = 700.;

/// Below which window width a toolbar button is left out, to make room (TS's `hideBelow`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HideBelow {
    /// [`SMALL`].
    Small,
    /// `views::repository::LARGE`.
    Large,
}

impl HideBelow {
    fn width(self) -> f32 {
        match self {
            HideBelow::Small => SMALL,
            HideBelow::Large => LARGE,
        }
    }
}

type OnClick = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;
type OnDismiss = Rc<dyn Fn(&mut Window, &mut App) + 'static>;

/// Why the button's action failed, shown under it until dismissed (see `FailurePopover`).
struct Failure {
    title: SharedString,
    error: Option<DataError>,
    on_dismiss: OnDismiss,
}

/// One of the repository toolbar's actions: an icon over its label, with a count in the icon's
/// corner if it has one (how many commits there are to pull, say), and why its action failed
/// under it, if it did. Its label is its accessible name; its tooltip says more (`title`), e.g.
/// what Pull would pull. Below the `md` breakpoint the label goes, and below `hide_below` the
/// whole button.
///
/// Running (`busy`), a spinner takes the icon's place and clicks are ignored; it isn't faded, as
/// it's still going on. Disabled, it's faded. It's `Selectable`, so it can open a popover, which
/// shows it pressed while that's open.
#[derive(IntoElement)]
pub struct ToolbarButton {
    id: ElementId,
    icon: IconName,
    label: SharedString,
    title: Option<SharedString>,
    accent: bool,
    danger: bool,
    hide_below: Option<HideBelow>,
    disabled: bool,
    busy: bool,
    selected: bool,
    count: Option<u64>,
    failure: Option<Failure>,
    on_click: Option<OnClick>,
}

impl ToolbarButton {
    pub fn new(id: impl Into<ElementId>, icon: IconName, label: impl Into<SharedString>) -> Self {
        ToolbarButton {
            id: id.into(),
            icon,
            label: label.into(),
            title: None,
            accent: false,
            danger: false,
            hide_below: None,
            disabled: false,
            busy: false,
            selected: false,
            count: None,
            failure: None,
            on_click: None,
        }
    }

    /// What its tooltip says; the label by default.
    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// In the accent color, as Pull is.
    pub fn accent(mut self, accent: bool) -> Self {
        self.accent = accent;
        self
    }

    /// In the danger color, as Fetch is once it failed.
    pub fn danger(mut self, danger: bool) -> Self {
        self.danger = danger;
        self
    }

    pub fn hide_below(mut self, width: HideBelow) -> Self {
        self.hide_below = Some(width);
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Its action is running: a spinner takes the icon's place, and it can't be clicked.
    pub fn busy(mut self, busy: bool) -> Self {
        self.busy = busy;
        self
    }

    /// A count in the icon's corner; none when 0.
    pub fn count(mut self, count: Option<u64>) -> Self {
        self.count = count.filter(|count| *count > 0);
        self
    }

    /// Says why its action failed, titled `title`, while there's an `error`; `on_dismiss`
    /// forgets it.
    pub fn failure(
        mut self,
        title: impl Into<SharedString>,
        error: Option<DataError>,
        on_dismiss: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.failure = Some(Failure {
            title: title.into(),
            error,
            on_dismiss: Rc::new(on_dismiss),
        });
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }

    pub fn label(&self) -> &SharedString {
        &self.label
    }

    /// What its tooltip says.
    pub fn tooltip(&self) -> &SharedString {
        self.title.as_ref().unwrap_or(&self.label)
    }

    pub fn is_disabled(&self) -> bool {
        self.disabled
    }

    pub fn is_busy(&self) -> bool {
        self.busy
    }

    pub fn shown_count(&self) -> Option<u64> {
        self.count
    }

    /// Why its action failed, if that's shown.
    pub fn shown_failure(&self) -> Option<&DataError> {
        self.failure
            .as_ref()
            .and_then(|failure| failure.error.as_ref())
    }

    /// Whether it's left out at a window `width` wide.
    pub fn is_hidden_at(&self, width: f32) -> bool {
        self.hide_below.is_some_and(|below| width < below.width())
    }
}

impl Selectable for ToolbarButton {
    fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    fn is_selected(&self) -> bool {
        self.selected
    }
}

impl RenderOnce for ToolbarButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = *cx.palette();
        let width = f32::from(window.viewport_size().width);
        if self.is_hidden_at(width) {
            return div().into_any_element();
        }
        let with_label = width >= MEDIUM;
        let tooltip = self.tooltip().clone();
        let color = if self.danger {
            palette.coral
        } else if self.accent {
            palette.blue
        } else {
            palette.muted
        };
        // Faded when it can't be used, but not while it's busy: that's still going on.
        let faded = self.disabled && !self.busy;
        let icon = if self.busy {
            Spinner::new()
                .icon(Icon::new(IconName::LoaderCircle))
                .color(color)
                .small()
                .into_any_element()
        } else {
            Icon::new(self.icon).size_4().into_any_element()
        };
        let count = self.count.map(|count| {
            div()
                .absolute()
                .top_0p5()
                .right_0p5()
                .min_w(rems(1.))
                .px_1()
                .rounded_full()
                .bg(palette.blue)
                .text_color(palette.panel)
                .text_size(rems(0.63))
                .line_height(rems(1.))
                .font_weight(FontWeight::BOLD)
                .flex()
                .justify_center()
                .child(count.to_string())
        });
        let button = Button::new(self.id.clone())
            .ghost()
            .small()
            .relative()
            .h_10()
            .px_1p5()
            .when(with_label, |this| this.min_w(rems(2.87)))
            .when(!with_label, |this| this.min_w_9())
            .text_color(color)
            .accessibility_label(self.label.clone())
            .tooltip(tooltip)
            .selected(self.selected)
            .disabled(self.disabled && !self.busy)
            .loading(self.busy)
            .when(faded, |this| this.opacity(0.5))
            .child(
                v_flex()
                    .items_center()
                    .gap_0p5()
                    .child(icon)
                    .when(with_label, |this| {
                        this.child(div().text_size(rems(0.7)).child(self.label.clone()))
                    }),
            )
            .children(count)
            .when_some(self.on_click, |button, on_click| {
                button.on_click(move |event, window, cx| on_click(event, window, cx))
            });
        match self.failure {
            Some(failure) => {
                let on_dismiss = failure.on_dismiss;
                FailurePopover::new(
                    SharedString::from(format!("{}-failure", self.id)),
                    failure.title,
                    failure.error,
                    button,
                )
                .on_dismiss(move |window, cx| on_dismiss(window, cx))
                .into_any_element()
            }
            None => button.into_any_element(),
        }
    }
}
