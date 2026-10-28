//! **Stub.** The repository's branches, tags, stashes and worktrees. Replaces
//! `components/repository/sidebar/refs-sidebar.tsx`.

use gitto_git::input::RepositoryId;
use gpui_kit::prelude::*;
use gpui_kit::{Context, Entity, Window, div};

use crate::components::placeholder::Placeholder;
use crate::components::repository::branch_menu::BranchMenuProvider;
use crate::theme::PaletteExt as _;

/// The repository's local branches, remotes, tags, stashes and worktrees, in collapsible
/// sections (see `state::collapsed`); below 900px a rail of icons. An entity: it loads the refs,
/// the stashes and the worktrees, and keeps what's collapsed. The page tells it whether it's open
/// ([`set_open`](Self::set_open)); its width is the page's.
///
/// Stub: renders a placeholder.
pub struct RefsSidebar {
    repository_id: RepositoryId,
    /// What its branches' menus are built from (see `branch_menu`); the stub has none.
    _branch_menu: Entity<BranchMenuProvider>,
    open: bool,
}

impl RefsSidebar {
    pub fn new(
        repository_id: RepositoryId,
        branch_menu: Entity<BranchMenuProvider>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Self {
        RefsSidebar {
            repository_id,
            _branch_menu: branch_menu,
            open: true,
        }
    }

    pub fn repository_id(&self) -> &RepositoryId {
        &self.repository_id
    }

    pub fn set_repository(&mut self, repository_id: RepositoryId, cx: &mut Context<Self>) {
        if self.repository_id != repository_id {
            self.repository_id = repository_id;
            cx.notify();
        }
    }

    pub fn set_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.open != open {
            self.open = open;
            cx.notify();
        }
    }
}

impl Render for RefsSidebar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(cx.palette().panel)
            .p_2()
            .child(Placeholder::new("Refs sidebar", "sidebar/refs-sidebar.tsx"))
    }
}
