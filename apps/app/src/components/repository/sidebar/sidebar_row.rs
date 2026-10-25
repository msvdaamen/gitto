//! The sidebar's lines: a folder of a ref tree, or an item (a ref, a stash, a worktree). Ported
//! from `components/repository/sidebar/sidebar-row.tsx`.

use std::rc::Rc;

use gpui_kit::component::h_flex;
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::*;
use gpui_kit::{
    App, Context, ElementId, Rems, Role, SharedString, TestSupportExt as _, Window, div,
};

use super::base_px;
use gitto_app_kit::theme::PaletteExt as _;
use gitto_app_kit::ui::{Icon, IconName, Tone, menu_frame};

/// Space below every row in a section.
const ROW_GAP: f32 = 1.;
/// Height of every row in a section, including the gap below it, in pixels of the 15px base.
pub const ROW_PX: f32 = 28. + ROW_GAP;
/// [`ROW_PX`], in rems.
pub const ROW_HEIGHT: Rems = base_px(ROW_PX);
/// A row's own height, from [`ROW_HEIGHT`] so the two can't drift apart.
const ROW_BOX: Rems = base_px(ROW_PX - ROW_GAP);
/// Indentation per tree level (12px).
const DEPTH_INDENT: f32 = 12.;
/// The gap between a row's columns (5px).
const COLUMN_GAP: Rems = base_px(5.);

/// What a row does when it's used: double-clicked, or a folder toggled.
pub type RowAction = Rc<dyn Fn(&mut Window, &mut App) + 'static>;
/// What a row's menu holds, built when it's right-clicked (see `components::ui::context_menu`).
pub type RowMenuBuilder =
    Rc<dyn Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static>;

/// A row's clickable frame: full width, its own height, rounded, muted until hovered.
fn row_frame(
    id: ElementId,
    padding_left: Rems,
    cx: &App,
) -> impl Styled + ParentElement + StatefulInteractiveElement + IntoElement + 'static {
    let palette = *cx.palette();
    h_flex()
        .id(id)
        .test_support()
        .role(Role::Button)
        .w_full()
        .h(ROW_BOX)
        .gap(COLUMN_GAP)
        .pl(padding_left)
        .pr(base_px(7.))
        .rounded_md()
        .text_color(palette.muted)
        .hover(|style| style.bg(palette.panel_hover).text_color(palette.text_soft))
}

/// A folder in a ref tree, which collapses the refs below it: a chevron, a folder (open while
/// it's expanded), its name and how many refs are below it. Clicking it toggles it.
#[derive(IntoElement)]
pub struct SidebarFolder {
    id: ElementId,
    name: SharedString,
    count: usize,
    depth: usize,
    collapsed: bool,
    on_toggle: Option<RowAction>,
}

impl SidebarFolder {
    pub fn new(
        id: impl Into<ElementId>,
        name: impl Into<SharedString>,
        count: usize,
        depth: usize,
        collapsed: bool,
    ) -> Self {
        SidebarFolder {
            id: id.into(),
            name: name.into(),
            count,
            depth,
            collapsed,
            on_toggle: None,
        }
    }

    pub fn on_toggle(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_toggle = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for SidebarFolder {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = *cx.palette();
        let padding = base_px(8. + self.depth as f32 * DEPTH_INDENT);
        let name = self.name.clone();
        row_frame(self.id, padding, cx)
            .aria_label(self.name.clone())
            .aria_expanded(!self.collapsed)
            .tooltip(move |window, cx| Tooltip::new(name.clone()).build(window, cx))
            .when_some(self.on_toggle, |row, on_toggle| {
                row.on_click(move |_, window, cx| on_toggle(window, cx))
            })
            .child(
                Icon::new(if self.collapsed {
                    IconName::ChevronRight
                } else {
                    IconName::ChevronDown
                })
                .size(base_px(12.)),
            )
            .child(
                Icon::new(if self.collapsed {
                    IconName::Folder
                } else {
                    IconName::FolderOpen
                })
                .size(base_px(13.)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(base_px(12.5))
                    .child(self.name),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(base_px(10.5))
                    .text_color(palette.faint)
                    .child(self.count.to_string()),
            )
    }
}

/// A small icon after a row's label, saying `label` to assistive tech: e.g. that a branch is
/// checked out in a worktree.
#[derive(Debug, Clone, PartialEq)]
pub struct RowTag {
    pub icon: IconName,
    pub label: SharedString,
}

/// An item in a section: a ref, a stash or a worktree. Its icon, its label, then whichever of a
/// tag, a meta (a branch's ahead count, say) and a count it has: the label takes the space left,
/// and the meta what it needs, up to half the row, as a worktree's branch can be long. Its tooltip is its `title`, or its label. Active, it stands out; highlighted (its menu
/// open), it shows as if hovered; faded (a worktree whose folder is gone), it's dimmed.
#[derive(IntoElement)]
pub struct SidebarRow {
    id: ElementId,
    icon: IconName,
    label: SharedString,
    title: Option<SharedString>,
    depth: usize,
    active: bool,
    highlighted: bool,
    tag: Option<RowTag>,
    count: Option<u64>,
    meta: Option<SharedString>,
    faded: bool,
    on_double_click: Option<RowAction>,
    menu: Option<RowMenuBuilder>,
}

impl SidebarRow {
    pub fn new(id: impl Into<ElementId>, icon: IconName, label: impl Into<SharedString>) -> Self {
        SidebarRow {
            id: id.into(),
            icon,
            label: label.into(),
            title: None,
            depth: 0,
            active: false,
            highlighted: false,
            tag: None,
            count: None,
            meta: None,
            faded: false,
            on_double_click: None,
            menu: None,
        }
    }

    /// What its tooltip says; the label by default.
    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// How deep in a tree it is.
    pub fn depth(mut self, depth: usize) -> Self {
        self.depth = depth;
        self
    }

    /// The one in use, e.g. the checked-out branch.
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    /// Shown as if hovered, e.g. while its menu is open.
    pub fn highlighted(mut self, highlighted: bool) -> Self {
        self.highlighted = highlighted;
        self
    }

    pub fn tag(mut self, tag: Option<RowTag>) -> Self {
        self.tag = tag;
        self
    }

    pub fn count(mut self, count: Option<u64>) -> Self {
        self.count = count;
        self
    }

    pub fn meta(mut self, meta: Option<SharedString>) -> Self {
        self.meta = meta;
        self
    }

    /// Dimmed, e.g. a worktree whose folder is gone.
    pub fn faded(mut self, faded: bool) -> Self {
        self.faded = faded;
        self
    }

    pub fn on_double_click(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_double_click = Some(Rc::new(handler));
        self
    }

    /// The menu opened by right-clicking it.
    pub fn menu(
        mut self,
        builder: impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static,
    ) -> Self {
        self.menu = Some(Rc::new(builder));
        self
    }

    /// What its tooltip says.
    pub fn tooltip(&self) -> &SharedString {
        self.title.as_ref().unwrap_or(&self.label)
    }
}

impl RenderOnce for SidebarRow {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = *cx.palette();
        let tooltip = self.tooltip().clone();
        let padding = base_px(25. + self.depth as f32 * DEPTH_INDENT);
        let (count_bg, count_text) = Tone::Neutral.colors(&palette);
        let icon = Icon::new(self.icon)
            .size(base_px(13.))
            .flex_none()
            .when(self.active, |icon| icon.text_color(palette.primary_strong));
        let row = row_frame(self.id, padding, cx)
            .aria_label(self.label.clone())
            .aria_selected(self.active)
            // The active one stands out already.
            .when(self.active, |row| {
                row.bg(palette.primary_soft).text_color(palette.text)
            })
            .when(!self.active && self.highlighted, |row| {
                row.bg(palette.panel_hover).text_color(palette.text_soft)
            })
            .when(self.faded, |row| row.opacity(0.6))
            // Not over its menu.
            .when(!self.highlighted, |row| {
                row.tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
            })
            .when_some(self.on_double_click, |row, on_double_click| {
                row.on_click(move |event, window, cx| {
                    if event.click_count() == 2 {
                        on_double_click(window, cx);
                    }
                })
            })
            .child(icon)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(base_px(12.5))
                    .child(self.label),
            )
            .when_some(self.tag, |row, tag| {
                row.child(
                    div()
                        .id("tag")
                        .test_support()
                        .flex_none()
                        .role(Role::Image)
                        .aria_label(tag.label)
                        .text_color(palette.faint)
                        .child(Icon::new(tag.icon).size(base_px(11.))),
                )
            })
            .when_some(self.meta, |row, meta| {
                row.child(
                    div()
                        .flex_shrink(1.)
                        .min_w_0()
                        .max_w_1_2()
                        .truncate()
                        .text_size(base_px(10.5))
                        .text_color(palette.blue)
                        .child(meta),
                )
            })
            .when_some(self.count, |row, count| {
                row.child(
                    div()
                        .flex_none()
                        .min_w(base_px(17.))
                        .px_1()
                        .py_0p5()
                        .rounded_lg()
                        .bg(count_bg)
                        .text_color(count_text)
                        .text_center()
                        .text_size(base_px(10.5))
                        .child(count.to_string()),
                )
            });
        match self.menu {
            Some(menu) => row
                .context_menu(move |popup, window, cx| menu(menu_frame(popup), window, cx))
                .into_any_element(),
            None => row.into_any_element(),
        }
    }
}
