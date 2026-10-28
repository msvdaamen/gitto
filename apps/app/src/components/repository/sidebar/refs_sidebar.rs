//! The repository's sidebar. Ported from `components/repository/sidebar/refs-sidebar.tsx`.

use gitto_git::input::RepositoryId;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Selectable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{Context, Entity, SharedString, Window, div};

use super::agent_placeholder::AgentPlaceholder;
use super::ref_list::RefList;
use crate::components::ui::{Icon, IconName, SegmentedControl};
use crate::theme::{PaletteExt as _, base_px};
use crate::views::repository::MEDIUM;

/// What the sidebar shows: the refs, or the agents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarMode {
    List,
    Agents,
}

impl SidebarMode {
    pub fn label(self) -> &'static str {
        match self {
            SidebarMode::List => "List",
            SidebarMode::Agents => "Agents",
        }
    }

    fn icon(self) -> IconName {
        match self {
            SidebarMode::List => IconName::ListTree,
            SidebarMode::Agents => IconName::Bot,
        }
    }
}

/// The views the sidebar switches between. The Agents view is hidden until agent sessions exist;
/// adding it back here brings back the switch.
pub const MODES: &[SidebarMode] = &[SidebarMode::List];

/// Height of the Configure sidebar button along the bottom, which the list leaves room for.
const FOOTER_HEIGHT: f32 = 34.;

/// The repository's local branches, remotes, tags, stashes and worktrees, in collapsible
/// sections (see [`RefList`]); below 900px a rail of icons, each opening its section in a
/// popover. Along the bottom, a button to configure it (which does nothing yet). An entity: it
/// keeps the list, and which view is on show. The page tells it whether it's open
/// ([`set_open`](Self::set_open)), fading it out while it's not; its width is the page's.
pub struct RefsSidebar {
    list: Entity<RefList>,
    mode: SidebarMode,
    open: bool,
}

impl RefsSidebar {
    pub fn new(repository_id: RepositoryId, window: &mut Window, cx: &mut Context<Self>) -> Self {
        RefsSidebar {
            list: cx.new(|cx| RefList::new(repository_id, window, cx)),
            mode: SidebarMode::List,
            open: true,
        }
    }

    pub fn repository_id<'a>(&self, cx: &'a gpui_kit::App) -> &'a RepositoryId {
        self.list.read(cx).repository_id()
    }

    /// Shows another repository's (see `RefList::set_repository`).
    pub fn set_repository(
        &mut self,
        repository_id: RepositoryId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.list.update(cx, |list, cx| {
            list.set_repository(repository_id, window, cx)
        });
    }

    pub fn set_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.open != open {
            self.open = open;
            cx.notify();
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    fn set_mode(&mut self, mode: SidebarMode, cx: &mut Context<Self>) {
        if self.mode != mode {
            self.mode = mode;
            cx.notify();
        }
    }

    /// The List / Agents switch: a segmented control, or a column of icons in the rail.
    fn render_mode_switch(&self, rail: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = *cx.palette();
        let this = cx.entity().downgrade();
        if !rail {
            return div()
                .px(base_px(10.))
                .pt(base_px(9.))
                .pb_2()
                .child(
                    SegmentedControl::new(
                        "sidebar-mode",
                        self.mode.label(),
                        MODES.iter().map(|mode| mode.label()),
                    )
                    .on_change(move |label, _, cx| {
                        if let Some(mode) = MODES.iter().find(|mode| mode.label() == label.as_ref())
                        {
                            let mode = *mode;
                            let _ = this.update(cx, |sidebar, cx| sidebar.set_mode(mode, cx));
                        }
                    }),
                )
                .into_any_element();
        }
        v_flex()
            .items_center()
            .gap_1()
            .py_2()
            .border_b_1()
            .border_color(palette.border_soft)
            .children(MODES.iter().map(|mode| {
                let mode = *mode;
                let this = this.clone();
                Button::new(SharedString::from(format!("sidebar-mode-{}", mode.label())))
                    .ghost()
                    .size(base_px(36.))
                    .rounded_lg()
                    .icon(Icon::new(mode.icon()))
                    .accessibility_label(mode.label())
                    .tooltip(mode.label())
                    .selected(self.mode == mode)
                    .on_click(move |_, _, cx| {
                        let _ = this.update(cx, |sidebar, cx| sidebar.set_mode(mode, cx));
                    })
            }))
            .into_any_element()
    }
}

impl Render for RefsSidebar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = *cx.palette();
        let rail = f32::from(window.viewport_size().width) < MEDIUM;
        let content = match self.mode {
            SidebarMode::List => div()
                .flex_1()
                .min_h_0()
                .overflow_hidden()
                .pb(base_px(FOOTER_HEIGHT + 1.))
                .child(self.list.clone())
                .into_any_element(),
            SidebarMode::Agents => AgentPlaceholder::new(rail).into_any_element(),
        };
        let configure = Button::new("configure-sidebar")
            .ghost()
            .xsmall()
            .when(!rail, |button| button.label("Configure sidebar"))
            .icon(Icon::new(IconName::Settings))
            .accessibility_label("Configure sidebar")
            .text_color(palette.faint);
        v_flex()
            .id("refs-sidebar")
            .relative()
            .size_full()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .bg(palette.panel)
            .when(!self.open, |sidebar| sidebar.opacity(0.))
            .when(MODES.len() > 1, |sidebar| {
                sidebar.child(self.render_mode_switch(rail, cx))
            })
            .child(content)
            .child(
                h_flex()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(base_px(FOOTER_HEIGHT))
                    .px_1p5()
                    .when(rail, |footer| footer.justify_center())
                    .border_t_1()
                    .border_color(palette.border_soft)
                    .bg(palette.panel)
                    .child(configure),
            )
    }
}

#[cfg(test)]
mod tests {
    use gitto_git::features::refs::schema::{Ref, RefKind};
    use gpui_kit::test::{TestQueryExt as _, TestWindowExt as _};
    use gpui_kit::{AnyWindowHandle, TestAppContext, px, size};

    use super::*;
    use crate::components::repository::test_support::{
        repository_id, set_refs, set_stashes, set_status, set_worktrees, setup, status,
    };

    fn local(name: &str, current: bool) -> Ref {
        Ref {
            name: name.into(),
            full_name: format!("refs/heads/{name}"),
            kind: RefKind::Local,
            sha: "abc".into(),
            current,
            upstream: None,
            ahead: 0,
            behind: 0,
        }
    }

    fn render(
        cx: &mut TestAppContext,
    ) -> (tempfile::TempDir, AnyWindowHandle, Entity<RefsSidebar>) {
        let dir = setup(cx);
        cx.update(|cx| cx.set_reduce_motion(true));
        let id = repository_id();
        set_status(&id, status(), cx);
        set_refs(&id, vec![local("main", true), local("feature", false)], cx);
        set_stashes(&id, vec![], cx);
        set_worktrees(&id, vec![], cx);
        let (window, sidebar) =
            crate::test_support::open(cx, move |window, cx| RefsSidebar::new(id, window, cx));
        (dir, window, sidebar)
    }

    fn has(window: AnyWindowHandle, id: &'static str, cx: &mut TestAppContext) -> bool {
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.try_find(id).is_some()
        })
        .unwrap()
    }

    fn click(window: AnyWindowHandle, id: &'static str, cx: &mut TestAppContext) {
        cx.update_window(window, |_, window, cx| window.click(id, cx))
            .unwrap();
        cx.run_until_parked();
    }

    #[gpui_kit::test]
    fn shows_the_sections_with_a_button_to_configure_the_sidebar(cx: &mut TestAppContext) {
        let (_dir, window, sidebar) = render(cx);
        assert!(has(window, "section-local", cx));
        assert!(has(window, "branch-refs/heads/feature", cx));
        assert!(!has(window, "rail-local", cx));
        cx.update_window(window, |_, window, _| {
            assert!(window.try_find_by_label("Configure sidebar").is_some());
        })
        .unwrap();
        // Only one view, so no switch between them.
        assert!(!has(window, "sidebar-mode", cx));

        sidebar.update(cx, |sidebar, cx| sidebar.set_open(false, cx));
        assert!(!sidebar.read_with(cx, |sidebar, _| sidebar.is_open()));
    }

    #[gpui_kit::test]
    fn is_a_rail_of_icons_below_900px_each_opening_its_section_beside_it(cx: &mut TestAppContext) {
        let (_dir, window, _) = render(cx);
        cx.simulate_window_resize(window, size(px(760.), px(800.)));
        cx.run_until_parked();
        assert!(has(window, "rail-local", cx));
        assert!(!has(window, "section-local", cx));
        assert!(!has(window, "branch-refs/heads/feature", cx));
        cx.update_window(window, |_, window, _| {
            assert_eq!(
                window.find("rail-local").label(),
                Some("Local branches (2)")
            );
        })
        .unwrap();

        click(window, "rail-local", cx);
        assert!(has(window, "branch-refs/heads/feature", cx));

        // The icon again closes it.
        click(window, "rail-local", cx);
        assert!(!has(window, "branch-refs/heads/feature", cx));
    }
}
