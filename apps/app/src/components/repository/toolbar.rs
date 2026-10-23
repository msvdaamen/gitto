//! **Stub.** The repository's toolbar. Replaces `components/repository/toolbar.tsx`.

use gitto_git::input::RepositoryId;
use gpui_kit::component::h_flex;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::prelude::*;
use gpui_kit::{Context, Entity, EventEmitter, SharedString, Subscription, Window, div, rems};

use super::branch_button::BranchButton;
use super::pull_button::PullButton;
use super::push_button::PushButton;
use super::stash_buttons::StashButtons;
use super::toolbar_button::{HideBelow, ToolbarButton};
use crate::components::ui::{Divider, IconButton, IconName, TextInput};
use crate::theme::PaletteExt as _;

/// The toolbar's height (TS's grid row, 49px).
pub const TOOLBAR_HEIGHT: f32 = 3.25;

/// What the toolbar asks of the page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolbarEvent {
    ToggleSidebar,
    ToggleDetails,
    /// The history's search changed.
    Search(SharedString),
}

/// The repository's toolbar: the sidebar's toggle and the current branch, undo and redo, Pull,
/// Push, Branch, the stash's buttons and Fetch, then the history's search and the details'
/// toggle. An entity: it keeps the search box and the buttons. The page tells it which panels are
/// open ([`set_panels`](Self::set_panels)) and handles its [`ToolbarEvent`]s.
///
/// Stub: the toggles and the search work; the current branch is a placeholder, and the buttons
/// are their stubs, disabled.
pub struct RepositoryToolbar {
    repository_id: RepositoryId,
    search: Entity<InputState>,
    sidebar_open: bool,
    details_open: bool,
    pull: Entity<PullButton>,
    push: Entity<PushButton>,
    branch: Entity<BranchButton>,
    stash: Entity<StashButtons>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ToolbarEvent> for RepositoryToolbar {}

impl RepositoryToolbar {
    pub fn new(repository_id: RepositoryId, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search commits…"));
        let subscription = cx.subscribe(&search, |_, search, event, cx| {
            if matches!(event, InputEvent::Change) {
                let value = search.read(cx).value();
                cx.emit(ToolbarEvent::Search(value));
            }
        });
        let id = repository_id.clone();
        RepositoryToolbar {
            pull: cx.new(|cx| PullButton::new(id.clone(), window, cx)),
            push: cx.new(|cx| PushButton::new(id.clone(), window, cx)),
            branch: cx.new(|cx| BranchButton::new(id.clone(), window, cx)),
            stash: cx.new(|cx| StashButtons::new(id, window, cx)),
            repository_id,
            search,
            sidebar_open: true,
            details_open: true,
            _subscriptions: vec![subscription],
        }
    }

    pub fn repository_id(&self) -> &RepositoryId {
        &self.repository_id
    }

    /// Shows another repository's.
    pub fn set_repository(&mut self, repository_id: RepositoryId, cx: &mut Context<Self>) {
        if self.repository_id == repository_id {
            return;
        }
        self.repository_id = repository_id.clone();
        let id = &repository_id;
        self.pull
            .update(cx, |button, cx| button.set_repository(id.clone(), cx));
        self.push
            .update(cx, |button, cx| button.set_repository(id.clone(), cx));
        self.branch
            .update(cx, |button, cx| button.set_repository(id.clone(), cx));
        self.stash
            .update(cx, |button, cx| button.set_repository(id.clone(), cx));
        cx.notify();
    }

    /// Which panels are open, for the toggles to show.
    pub fn set_panels(&mut self, sidebar_open: bool, details_open: bool, cx: &mut Context<Self>) {
        if (self.sidebar_open, self.details_open) != (sidebar_open, details_open) {
            self.sidebar_open = sidebar_open;
            self.details_open = details_open;
            cx.notify();
        }
    }
}

impl Render for RepositoryToolbar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = *cx.palette();
        h_flex()
            .h(rems(TOOLBAR_HEIGHT))
            .flex_none()
            .min_w_0()
            .gap_2()
            .px_2()
            .border_b_1()
            .border_color(palette.border)
            .bg(palette.panel)
            .child(
                IconButton::new("toggle-sidebar", IconName::PanelLeft, "Toggle sidebar")
                    .active(self.sidebar_open)
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::ToggleSidebar))),
            )
            .child(
                div()
                    .w_40()
                    .text_sm()
                    .text_color(palette.faint)
                    .child("Current branch"),
            )
            .child(ToolbarButton::new("undo", IconName::Undo2, "Undo").hide_below(HideBelow::Large))
            .child(ToolbarButton::new("redo", IconName::Redo2, "Redo").hide_below(HideBelow::Large))
            .child(Divider::new().tall())
            .child(self.pull.clone())
            .child(self.push.clone())
            .child(self.branch.clone())
            .child(self.stash.clone())
            .child(ToolbarButton::new("fetch", IconName::RefreshCw, "Fetch").disabled(true))
            .child(div().flex_1())
            .child(
                TextInput::new(&self.search)
                    .icon(IconName::Search)
                    .compact(true),
            )
            .child(
                IconButton::new("toggle-details", IconName::PanelRight, "Toggle details")
                    .active(self.details_open)
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(ToolbarEvent::ToggleDetails))),
            )
    }
}
