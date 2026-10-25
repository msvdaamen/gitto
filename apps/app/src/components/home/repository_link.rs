//! A row that opens a repository, as the home page's `<Link to="/$repoId">`s do.

use gitto_git::input::RepositoryId;
use gpui_kit::base::ObservedElement;
use gpui_kit::prelude::*;
use gpui_kit::{
    App, Div, ElementId, KeyDownEvent, Role, Stateful, TestSupportExt as _, Window, div,
};

use gitto_app_data::state::router::{Route, Router};

/// A link to the repository with `repository`'s id, as the element `id`: Tab stops at it, and a
/// click, Enter or Space opens the repository. Its look is the caller's.
pub fn repository_link(
    id: ElementId,
    repository: &str,
    window: &mut Window,
    cx: &mut App,
) -> ObservedElement<Stateful<Div>> {
    let focus = window
        .use_keyed_state(id.clone(), cx, |_, cx| cx.focus_handle().tab_stop(true))
        .read(cx)
        .clone();
    let open = repository.to_owned();
    let open_on_key = repository.to_owned();
    div()
        .id(id)
        .test_support()
        .track_focus(&focus)
        .role(Role::Link)
        .on_click(move |_, _, cx| open_repository(&open, cx))
        .on_key_down(move |event: &KeyDownEvent, _, cx| {
            if is_activation(event) {
                cx.stop_propagation();
                open_repository(&open_on_key, cx);
            }
        })
}

/// Opens the repository with `id`, as a link to it did.
fn open_repository(id: &str, cx: &mut App) {
    if let Ok(id) = RepositoryId::parse(id) {
        Router::navigate(Route::Repository(id), cx);
    }
}

/// Whether `event` activates a link-like row: Enter, or Space.
fn is_activation(event: &KeyDownEvent) -> bool {
    let modifiers = event.keystroke.modifiers;
    !modifiers.modified() && matches!(event.keystroke.key.as_str(), "enter" | "space")
}
