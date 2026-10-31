//! **Stub.** A repository toolbar button. Replaces `components/repository/stash-buttons.tsx`.

use gitto_git::input::RepositoryId;
use gpui_kit::component::h_flex;
use gpui_kit::prelude::*;
use gpui_kit::{Context, Window};

use super::toolbar_button::ToolbarButton;
use crate::components::ui::IconName;

/// Stash and Pop: stashes the uncommitted changes, and pops the latest stash. An entity: it shows its query's state and its operation's (see `crate::data`), for the
/// repository it's given. Stub: renders the button as it is while loading, disabled.
pub struct StashButtons {
    repository_id: RepositoryId,
}

impl StashButtons {
    pub fn new(repository_id: RepositoryId, _: &mut Window, _: &mut Context<Self>) -> Self {
        StashButtons { repository_id }
    }

    pub fn repository_id(&self) -> &RepositoryId {
        &self.repository_id
    }

    /// Shows another repository's.
    pub fn set_repository(&mut self, repository_id: RepositoryId, cx: &mut Context<Self>) {
        if self.repository_id != repository_id {
            self.repository_id = repository_id;
            cx.notify();
        }
    }
}

impl Render for StashButtons {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .gap_0p5()
            .child(ToolbarButton::new("stash", IconName::Archive, "Stash").disabled(true))
            .child(ToolbarButton::new("pop", IconName::ArchiveRestore, "Pop").disabled(true))
    }
}
