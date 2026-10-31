//! **Stub.** The repositories added to Gitto. Replaces `components/home/repository-list.tsx`.

use std::rc::Rc;

use gpui_kit::component::input::InputState;
use gpui_kit::prelude::*;
use gpui_kit::{App, Entity, Window, div};

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::h_flex;

use super::repository_card::RepositoryCard;
use super::summaries::RepositorySummary;
use crate::components::placeholder::Placeholder;
use crate::components::ui::{Icon, IconName, TextInput};

type Handler = Rc<dyn Fn(&mut Window, &mut App) + 'static>;

/// The repositories added to Gitto, with a search box to filter them (by name, path, branch and
/// remote), and an "Open repository" button while there are none. The search's state is the home
/// page's (`search`), so it's kept while the page is.
///
/// Stub: renders a placeholder with the search box and the "Open repository" button, and the
/// cards' stubs.
#[derive(IntoElement)]
pub struct RepositoryList {
    summaries: Rc<[RepositorySummary]>,
    /// Whether the repositories have loaded: until then, there's no telling there are none.
    loaded: bool,
    pub(crate) search: Entity<InputState>,
    pub(crate) on_open_repository: Handler,
}

impl RepositoryList {
    pub fn new(
        summaries: Rc<[RepositorySummary]>,
        loaded: bool,
        search: &Entity<InputState>,
        on_open_repository: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        RepositoryList {
            summaries,
            loaded,
            search: search.clone(),
            on_open_repository: Rc::new(on_open_repository),
        }
    }
}

impl RenderOnce for RepositoryList {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let on_open_repository = self.on_open_repository;
        div()
            .flex()
            .flex_col()
            .gap_1p5()
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        TextInput::new(&self.search)
                            .icon(IconName::Search)
                            .compact(true),
                    )
                    .child(
                        Button::new("open-repository-from-list")
                            .primary()
                            .icon(Icon::new(IconName::FolderOpen))
                            .label("Open repository")
                            .on_click(move |_, window, cx| on_open_repository(window, cx)),
                    ),
            )
            .child(div().h_16().child(
                Placeholder::new("Repository list", "components/home/repository-list.tsx").detail(
                    if self.loaded {
                        format!("{} repositories", self.summaries.len())
                    } else {
                        "Loading".to_owned()
                    },
                ),
            ))
            .children(
                self.summaries
                    .iter()
                    .map(|summary| RepositoryCard::new(summary.clone())),
            )
    }
}
