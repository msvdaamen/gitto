//! A row that opens a repository, as the home page's `<Link to="/$repoId">`s do.

use gitto_git::input::RepositoryId;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::prelude::*;
use gpui_kit::{App, ElementId};

use crate::state::router::{Route, Router};

/// A row that opens the repository with `repository`'s id, as the element `id`: a ghost button,
/// so Tab stops at it, and a click, Enter or Space opens the repository. Its look past that, and
/// what it shows (its one child, laid out by the caller), are the caller's.
pub fn repository_link(id: impl Into<ElementId>, repository: &str) -> Button {
    let repository = repository.to_owned();
    Button::new(id)
        .ghost()
        .h_auto()
        .justify_start()
        .on_click(move |_, _, cx| open_repository(&repository, cx))
}

/// Opens the repository with `id`, as a link to it did.
fn open_repository(id: &str, cx: &mut App) {
    if let Ok(id) = RepositoryId::parse(id) {
        Router::navigate(Route::Repository(id), cx);
    }
}
