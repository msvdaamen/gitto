//! **Stub.** A repository toolbar button. Replaces `components/repository/pull-button.tsx`.

use gitto_git::input::RepositoryId;
use gpui_kit::component::h_flex;
use gpui_kit::prelude::*;
use gpui_kit::{Context, Window};

use super::toolbar_button::ToolbarButton;
use crate::components::ui::IconName;

/// Pull: pulls the upstream into the checked-out branch, saying why when it can't (a detached HEAD, no upstream) and why it failed. An entity: it shows its query's state and its operation's (see `crate::data`), for the
/// repository it's given. Stub: renders the button as it is while loading, disabled.
pub struct PullButton {
    repository_id: RepositoryId,
}

impl PullButton {
    pub fn new(repository_id: RepositoryId, _: &mut Window, _: &mut Context<Self>) -> Self {
        PullButton { repository_id }
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

impl Render for PullButton {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .gap_0p5()
            .child(ToolbarButton::new("pull", IconName::Download, "Pull").disabled(true))
    }
}
