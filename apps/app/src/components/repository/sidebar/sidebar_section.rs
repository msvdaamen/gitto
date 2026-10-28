//! A section of the sidebar. Ported from `components/repository/sidebar/sidebar-section.tsx`.

use std::ops::Range;
use std::rc::Rc;

use gitto_ui_logic::format::compact_count;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{Selectable, Sizable as _, ThemeStyled as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, FontWeight, Role, SharedString, TestSupportExt as _, UniformListScrollHandle,
    Window, div,
};

use super::sidebar_row::{ROW_HEIGHT, ROW_PX, RowAction};
use gitto_app_data::data::DataError;
use gitto_app_kit::theme::{PaletteExt as _, base_px};
use gitto_app_kit::ui::{FailurePopover, Icon, IconName, VirtualList};

/// Height of a section's header.
const HEADER_HEIGHT: f32 = 30.;
/// Space above a section's first row and below its last one.
const LIST_PADDING: f32 = 4.;
/// Height of an expanded section without its rows: its top border, its header, the border below
/// that and the list's padding.
const SECTION_CHROME: f32 = 1. + HEADER_HEIGHT + 1. + 2. * LIST_PADDING;
/// The most a section's popover beside the rail lists before it scrolls, in pixels of the 15px
/// base, and as a share of the window's height (TS's `min(400px, 70vh)`): a short window would
/// cut off the bottom of a taller list.
const RAIL_LIST_MAX: f32 = 400.;
const RAIL_LIST_MAX_VIEWPORT: f32 = 0.7;
/// How wide a section's popover beside the rail is.
const RAIL_POPOVER_WIDTH: f32 = 260.;
/// The space between the rail's icon and its section's popover.
const RAIL_POPOVER_GAP: f32 = 10.;
/// The size of a section's icon in the rail.
const RAIL_ICON: f32 = 36.;

/// Renders a section's rows in view, by position (see `VirtualList`): side-effect-free.
pub type RenderRows = Rc<dyn Fn(Range<usize>, &mut Window, &mut App) -> Vec<AnyElement>>;

/// Why an action on a section's items failed, shown beside the section until dismissed.
pub struct SectionFailure {
    pub title: SharedString,
    pub error: Option<DataError>,
    pub on_dismiss: RowAction,
}

/// A section of the sidebar, like GitKraken's: its header always shows, and while expanded its
/// rows scroll on their own, only rendering the ones in view. Expanded sections share the
/// sidebar's height, but never take more than their rows need. In the rail (`rail`, below 900px)
/// it's an icon instead, with a badge counting its items, that opens its rows in a popover beside
/// it.
///
/// Busy (running something on its items, e.g. a pop of a stash), a spinner takes the icon's
/// place. Why that failed shows in a popover beside it ([`failure`](Self::failure)).
///
/// The header is a tab stop, which Enter or Space toggles as a click does; the rows are moved
/// through by the list around it, which has the focus for them (see `RefList`).
#[derive(IntoElement)]
pub struct SidebarSection {
    id: SharedString,
    title: SharedString,
    icon: IconName,
    busy: bool,
    count: usize,
    collapsed: bool,
    rail: bool,
    items: usize,
    render_rows: RenderRows,
    on_toggle: Option<RowAction>,
    scroll: Option<UniformListScrollHandle>,
    failure: Option<SectionFailure>,
}

impl SidebarSection {
    /// The section `id` (also what its parts' ids are made of), titled `title`, of `count`
    /// items, with `items` rows that `render_rows` renders.
    pub fn new(
        id: impl Into<SharedString>,
        title: impl Into<SharedString>,
        icon: IconName,
        count: usize,
        items: usize,
        render_rows: RenderRows,
    ) -> Self {
        SidebarSection {
            id: id.into(),
            title: title.into(),
            icon,
            busy: false,
            count,
            collapsed: false,
            rail: false,
            items,
            render_rows,
            on_toggle: None,
            scroll: None,
            failure: None,
        }
    }

    pub fn busy(mut self, busy: bool) -> Self {
        self.busy = busy;
        self
    }

    pub fn collapsed(mut self, collapsed: bool) -> Self {
        self.collapsed = collapsed;
        self
    }

    /// Shown in the narrow sidebar's rail.
    pub fn rail(mut self, rail: bool) -> Self {
        self.rail = rail;
        self
    }

    pub fn on_toggle(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_toggle = Some(Rc::new(handler));
        self
    }

    /// Follows its list's scroll position, for the owner to bring it back to the top in another
    /// repository.
    pub fn track_scroll(mut self, handle: &UniformListScrollHandle) -> Self {
        self.scroll = Some(handle.clone());
        self
    }

    pub fn failure(mut self, failure: SectionFailure) -> Self {
        self.failure = Some(failure);
        self
    }

    fn is_expanded(&self) -> bool {
        !self.collapsed && self.items > 0
    }

    fn render_header(&self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = *cx.palette();
        let on_toggle = self.on_toggle.clone();
        let id = SharedString::from(format!("section-{}", self.id));
        let focus = window
            .use_keyed_state(SharedString::from(format!("{id}-focus")), cx, |_, cx| {
                cx.focus_handle()
            })
            .read(cx)
            .clone();
        let focused = focus.is_focused(window);
        h_flex()
            .id(id)
            .test_support()
            .role(Role::Button)
            .track_focus(&focus)
            .tab_stop(true)
            .aria_label(SharedString::from(format!("{} {}", self.title, self.count)))
            .aria_expanded(!self.collapsed)
            .flex_none()
            .w_full()
            .h(base_px(HEADER_HEIGHT))
            .gap(base_px(6.))
            .px(base_px(10.))
            .bg(palette.panel_raised)
            .text_color(if self.collapsed {
                palette.muted
            } else {
                palette.text_soft
            })
            .hover(|style| style.bg(palette.panel_hover).text_color(palette.text_soft))
            .when_some(on_toggle, |header, on_toggle| {
                header.on_click(move |_, window, cx| on_toggle(window, cx))
            })
            .child(
                div().flex_none().text_color(palette.faint).child(
                    Icon::new(if self.collapsed {
                        IconName::ChevronRight
                    } else {
                        IconName::ChevronDown
                    })
                    .size(base_px(12.)),
                ),
            )
            .child(section_icon(self.icon, self.busy, 13., cx))
            .child(section_title(&self.title, self.count, cx))
            .when(focused, |header| header.focus_ring_style(window, cx))
    }

    /// Why an action failed, beside the section's top right corner (TS's `right-start`).
    fn render_failure(&mut self) -> Option<impl IntoElement + use<>> {
        let failure = self.failure.take()?;
        let on_dismiss = failure.on_dismiss;
        Some(
            div().absolute().top_0().right_0().size_0().child(
                FailurePopover::new(
                    SharedString::from(format!("{}-failure", self.id)),
                    failure.title,
                    failure.error.map(|error| error.to_string().into()),
                    FailureAnchor::default(),
                )
                .on_dismiss(move |window, cx| on_dismiss(window, cx)),
            ),
        )
    }

    fn render_wide(mut self, window: &mut Window, cx: &mut App) -> AnyElement {
        let palette = *cx.palette();
        let expanded = self.is_expanded();
        let failure = self.render_failure();
        let header = self.render_header(window, cx);
        v_flex()
            .id(SharedString::from(format!("sidebar-{}", self.id)))
            .relative()
            .min_w_0()
            // At least its top border and its header.
            .min_h(base_px(HEADER_HEIGHT + 1.))
            .border_t_1()
            .border_color(palette.border_soft)
            .when(expanded, |section| {
                section
                    .flex_1()
                    .max_h(base_px(SECTION_CHROME + self.items as f32 * ROW_PX))
            })
            .when(!expanded, |section| section.flex_none())
            .child(header)
            .when(expanded, |section| {
                section.child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .border_t_1()
                        .border_color(palette.border_soft)
                        .px_1p5()
                        .py(base_px(LIST_PADDING))
                        .child(
                            VirtualList::new(
                                SharedString::from(format!("{}-rows", self.id)),
                                self.items,
                                ROW_HEIGHT,
                                {
                                    let render_rows = Rc::clone(&self.render_rows);
                                    move |range, window, cx| render_rows(range, window, cx)
                                },
                            )
                            .when_some(self.scroll.as_ref(), |list, scroll| {
                                list.track_scroll(scroll)
                            }),
                        ),
                )
            })
            .children(failure)
            .into_any_element()
    }

    /// The section as an icon in the rail, with a badge counting its items, that opens its rows
    /// in a popover beside it.
    fn render_rail(mut self, cx: &App) -> AnyElement {
        let palette = *cx.palette();
        let failure = self.render_failure();
        let badge = (self.count > 0).then(|| {
            div()
                .absolute()
                .top(base_px(-2.))
                .right(base_px(-4.))
                .min_w(base_px(16.))
                .px(base_px(3.))
                .rounded_full()
                .border_2()
                .border_color(palette.panel)
                .bg(palette.panel_active)
                .text_center()
                .text_size(base_px(10.))
                .line_height(base_px(11.))
                .font_weight(FontWeight::BOLD)
                .text_color(palette.text_soft)
                .child(compact_count(self.count as u64))
        });
        let trigger = Button::new(SharedString::from(format!("rail-{}", self.id)))
            .ghost()
            .relative()
            .size(base_px(RAIL_ICON))
            .rounded_lg()
            .text_color(palette.muted)
            .accessibility_label(SharedString::from(format!(
                "{} ({})",
                self.title, self.count
            )))
            .tooltip(self.title.clone())
            .child(section_icon(self.icon, self.busy, 16., cx))
            .children(badge);
        let title = self.title.clone();
        let icon = self.icon;
        let count = self.count;
        let items = self.items;
        let render_rows = Rc::clone(&self.render_rows);
        let list_id = SharedString::from(format!("{}-rail-rows", self.id));
        // Beside the icon, its top level with the icon's (TS's `right-start`): Kit's popovers open
        // below their trigger, so it's moved up by the icon's height and right by its width and
        // the gap.
        let popover = Popover::new(SharedString::from(format!("rail-popover-{}", self.id)))
            .anchor(gpui_kit::Anchor::TopLeft)
            .offset(gpui_kit::px(0.))
            .mt(base_px(-RAIL_ICON))
            .ml(base_px(RAIL_ICON + RAIL_POPOVER_GAP))
            .p_0()
            .w(base_px(RAIL_POPOVER_WIDTH))
            .overflow_hidden()
            .trigger(trigger)
            .content(move |_, window, cx| {
                let palette = *cx.palette();
                let header = h_flex()
                    .flex_none()
                    .h(base_px(HEADER_HEIGHT))
                    .gap(base_px(6.))
                    .px(base_px(10.))
                    .border_b_1()
                    .border_color(palette.border_soft)
                    .bg(palette.panel_raised)
                    .text_color(palette.text_soft)
                    .child(Icon::new(icon).size(base_px(13.)).flex_none())
                    .child(section_title(&title, count, cx));
                let body = if items == 0 {
                    div()
                        .px_3()
                        .py_4()
                        .text_center()
                        .text_sm()
                        .text_color(palette.faint)
                        .child("Nothing here yet")
                        .into_any_element()
                } else {
                    let height =
                        base_px((items as f32 * ROW_PX + 2. * LIST_PADDING).min(RAIL_LIST_MAX))
                            .to_pixels(window.rem_size())
                            .min(window.viewport_size().height * RAIL_LIST_MAX_VIEWPORT);
                    let render_rows = Rc::clone(&render_rows);
                    div()
                        .h(height)
                        .px_1p5()
                        .py(base_px(LIST_PADDING))
                        .child(VirtualList::new(
                            list_id.clone(),
                            items,
                            ROW_HEIGHT,
                            move |range, window, cx| render_rows(range, window, cx),
                        ))
                        .into_any_element()
                };
                v_flex().w_full().child(header).child(body)
            });
        div()
            .id(SharedString::from(format!("sidebar-{}", self.id)))
            .relative()
            .flex_none()
            .child(popover)
            .children(failure)
            .into_any_element()
    }
}

impl RenderOnce for SidebarSection {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        if self.rail {
            self.render_rail(cx)
        } else {
            self.render_wide(window, cx)
        }
    }
}

/// A section's icon, or a spinner in its place while it's busy, `size` pixels.
fn section_icon(icon: IconName, busy: bool, size: f32, cx: &App) -> AnyElement {
    if busy {
        Spinner::new()
            .icon(Icon::new(IconName::LoaderCircle))
            .color(cx.palette().muted)
            .xsmall()
            .into_any_element()
    } else {
        Icon::new(icon)
            .size(base_px(size))
            .flex_none()
            .into_any_element()
    }
}

/// A section's title, with how many items it has.
fn section_title(title: &SharedString, count: usize, cx: &App) -> impl IntoElement + use<> {
    let palette = *cx.palette();
    h_flex()
        .flex_1()
        .min_w_0()
        .gap(base_px(6.))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(base_px(11.5))
                .font_weight(FontWeight(720.))
                .child(SharedString::from(title.to_uppercase())),
        )
        .child(
            div()
                .flex_none()
                .rounded_full()
                .bg(palette.bg)
                .px(base_px(6.))
                .py(base_px(1.))
                .text_size(base_px(10.5))
                .font_weight(FontWeight(650.))
                .text_color(palette.faint)
                .child(count.to_string()),
        )
}

/// What a section's failure popover is placed by: nothing to see, at the section's top right.
#[derive(IntoElement, Default)]
struct FailureAnchor {
    selected: bool,
}

impl Selectable for FailureAnchor {
    fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    fn is_selected(&self) -> bool {
        self.selected
    }
}

impl RenderOnce for FailureAnchor {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div().size_0()
    }
}
