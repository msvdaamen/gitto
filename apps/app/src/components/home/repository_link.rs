//! A row that opens a repository, as the home page's `<Link to="/$repoId">`s do.

use gitto_git::input::RepositoryId;
use gpui_kit::ElementId;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::prelude::*;

use gitto_app_data::state::router::{Route, Router};

/// A row that opens the repository `repository`, as the element `id`: a ghost button, so Tab
/// stops at it, and a click, Enter or Space opens the repository. Its look past that, and what it
/// shows (its one child, laid out by the caller), are the caller's.
pub fn repository_link(id: impl Into<ElementId>, repository: &RepositoryId) -> Button {
    let repository = repository.clone();
    Button::new(id)
        .ghost()
        .h_auto()
        .justify_start()
        .on_click(move |_, _, cx| Router::navigate(Route::Repository(repository.clone()), cx))
}
