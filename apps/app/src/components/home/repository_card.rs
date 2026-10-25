//! **Stub.** A repository in the home page's list. Replaces
//! `components/home/repository-card.tsx`.

use gitto_git::input::RepositoryId;
use gpui_kit::prelude::*;
use gpui_kit::{App, SharedString, Window, div};

use super::summaries::RepositorySummary;
use crate::components::placeholder::Placeholder;
use crate::state::router::{Route, Router};

/// A repository in the home page's list: where it's hosted, its branch, and what's changed. Opens
/// it when clicked. Stub: renders a placeholder that opens it.
#[derive(IntoElement)]
pub struct RepositoryCard {
    summary: RepositorySummary,
}

impl RepositoryCard {
    pub fn new(summary: RepositorySummary) -> Self {
        RepositoryCard { summary }
    }
}

impl RenderOnce for RepositoryCard {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let id = self.summary.repository.id.clone();
        div()
            .id(SharedString::from(format!("repository-card-{id}")))
            .h_20()
            .on_click(move |_, _, cx| {
                if let Ok(id) = RepositoryId::parse(id.clone()) {
                    Router::navigate(Route::Repository(id), cx);
                }
            })
            .child(
                Placeholder::new("Repository card", "components/home/repository-card.tsx")
                    .detail(self.summary.repository.name.clone()),
            )
    }
}
