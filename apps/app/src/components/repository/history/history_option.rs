//! What every row of the history shares. Ported from
//! `components/repository/history/history-option.tsx`.

use std::rc::Rc;

use gpui_kit::component::h_flex;
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Background, Context, Div, ElementId, FontWeight, Hsla, Role, SharedString,
    TestSupportExt as _, Window, div, px, rems,
};

use super::columns::{LABEL_ICON, LABEL_LINE, LABEL_TEXT, ROW_HEIGHT, mixed};
use crate::components::ui::{Icon, IconName, LineStats, Tone, menu_frame};
use crate::theme::PaletteExt as _;
use gitto_ui::git::changes::LineCounts;

/// A row's place in the list. Only the rows in view are rendered, so assistive tech needs to be
/// told how long the list is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowPosition {
    pub index: usize,
    pub count: usize,
}

/// Selects a row.
pub type OnSelect = Rc<dyn Fn(&mut Window, &mut App)>;
/// Builds a row's menu, as `.context_menu(…)` does.
pub type MenuBuilder = Rc<dyn Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu>;

/// How a row looks: its background, at rest and hovered, its cells' bottom border, and the accent
/// on its leading edge, if any.
#[derive(Clone)]
pub struct RowLook {
    pub background: Option<Background>,
    pub hover: Option<Hsla>,
    pub border: Hsla,
    pub dashed: bool,
    pub accent: Option<Hsla>,
    /// Its text, where a cell doesn't say otherwise.
    pub text: Hsla,
}

/// A row of the history table as a selectable option, laid out in the table's columns (each
/// cell sized with `Column::apply`). Its cells, but the graph's, are underlined: the graph's lines
/// run on from row to row, so it reads as one graph. Not focused itself: the list has the focus,
/// and moves the selection with the keyboard. The selected one is outlined while the keyboard does
/// (`outlined`). Its accessible name (`label`) is what it says, e.g. for a commit its message and
/// author: the author is otherwise only in the graph's tooltip.
#[derive(IntoElement)]
pub struct HistoryOption {
    id: ElementId,
    label: SharedString,
    position: RowPosition,
    selected: bool,
    outlined: bool,
    look: RowLook,
    cells: Vec<AnyElement>,
    on_select: OnSelect,
    menu: Option<MenuBuilder>,
}

impl HistoryOption {
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        position: RowPosition,
        look: RowLook,
        on_select: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        HistoryOption {
            id: id.into(),
            label: label.into(),
            position,
            selected: false,
            outlined: false,
            look,
            cells: Vec::new(),
            on_select: Rc::new(on_select),
            menu: None,
        }
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Outlined as the keyboard's: it's selected, and the list has the focus from the keyboard.
    pub fn outlined(mut self, outlined: bool) -> Self {
        self.outlined = outlined;
        self
    }

    /// A cell, underlined as the row's look has it.
    pub fn cell(mut self, cell: Div) -> Self {
        let look = &self.look;
        let cell = cell
            .border_b_1()
            .border_color(look.border)
            .when(look.dashed, |this| this.border_dashed());
        self.cells.push(cell.into_any_element());
        self
    }

    /// The graph's cell, which isn't underlined (see the type's doc).
    pub fn graph_cell(mut self, cell: Div) -> Self {
        self.cells.push(cell.into_any_element());
        self
    }

    /// The row's menu, opened by right-clicking it: e.g. a commit's (see `CommitMenu`).
    pub fn menu(
        mut self,
        menu: impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static,
    ) -> Self {
        self.menu = Some(Rc::new(menu));
        self
    }
}

impl RenderOnce for HistoryOption {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = *cx.palette();
        let on_select = self.on_select;
        let look = self.look;
        let row = h_flex()
            .id(self.id)
            .role(Role::ListBoxOption)
            .test_support()
            .aria_label(self.label)
            .aria_selected(self.selected)
            .aria_position_in_set(self.position.index + 1)
            .aria_size_of_set(self.position.count)
            .when(self.selected, |this| this.aria_active_descendant())
            .relative()
            .w_full()
            .h(ROW_HEIGHT)
            .items_stretch()
            .text_color(look.text)
            .when_some(look.background, |this, background| this.bg(background))
            .when_some(look.hover, |this, hover| {
                this.hover(move |style| style.bg(hover))
            })
            .on_click(move |_, window, cx| on_select(window, cx))
            .children(self.cells)
            .when_some(look.accent, |this, accent| {
                // The TS's `shadow-[inset_2px_0_…]`.
                this.child(
                    div()
                        .absolute()
                        .left_0()
                        .top_0()
                        .bottom_0()
                        .w(px(2.))
                        .bg(accent),
                )
            })
            .when(self.outlined, |this| {
                this.child(
                    div()
                        .absolute()
                        .inset_0()
                        .border_2()
                        .border_color(palette.primary),
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

/// What a row that isn't a commit is, e.g. "WIP" or "Stash", in its branch / tag column.
#[derive(IntoElement)]
pub struct RowKindBadge {
    icon: IconName,
    label: SharedString,
}

impl RowKindBadge {
    pub fn new(icon: IconName, label: impl Into<SharedString>) -> Self {
        RowKindBadge {
            icon,
            label: label.into(),
        }
    }
}

impl RenderOnce for RowKindBadge {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = *cx.palette();
        let (background, foreground) = Tone::Amber.colors(&palette);
        h_flex()
            .flex_none()
            .gap_1()
            .rounded_sm()
            .border_1()
            .border_color(mixed(palette.amber, 0.4, palette.border))
            .px(rems(5. / 15.))
            .py(rems(3. / 15.))
            .bg(background)
            .text_color(foreground)
            .text_size(LABEL_TEXT)
            .line_height(LABEL_LINE)
            .font_weight(FontWeight(680.))
            .child(Icon::new(self.icon).size(LABEL_ICON))
            .child(self.label)
    }
}

/// The lines a commit or stash added and removed, once its files are loaded. Only shown for the
/// selected row once the details show it, whose files are loaded anyway: for every row an arrow
/// key held down passes, it would run git once per row (see `HistoryTable`).
#[derive(IntoElement)]
pub struct RowTotals {
    totals: LineCounts,
}

impl RowTotals {
    pub fn new(totals: LineCounts) -> Self {
        RowTotals { totals }
    }
}

impl RenderOnce for RowTotals {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div()
            .flex_none()
            .text_size(LABEL_TEXT)
            .child(LineStats::new(self.totals.additions, self.totals.deletions))
    }
}
