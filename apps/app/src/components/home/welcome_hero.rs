//! **Stub.** The home page banner. Replaces `components/home/welcome-hero.tsx`.

use std::rc::Rc;

use gpui_kit::prelude::*;
use gpui_kit::{App, Window};

use super::summaries::RepositorySummary;
use crate::components::placeholder::Placeholder;
use crate::data::QueryError;

type Handler = Rc<dyn Fn(&mut Window, &mut App) + 'static>;

/// The home page banner: a greeting, a way to open a repository, and whether all are in sync.
/// Stub: renders a placeholder until it's built.
#[derive(IntoElement)]
pub struct WelcomeHero {
    /// The name the user commits with: `Some(None)` if they haven't set one, `None` until loaded.
    pub(crate) user_name: Option<Option<String>>,
    pub(crate) summaries: Rc<[RepositorySummary]>,
    /// Why the last repository couldn't be opened, under the button, until it's dismissed.
    pub(crate) open_error: Option<QueryError>,
    pub(crate) on_open_repository: Handler,
    pub(crate) on_dismiss_open_error: Handler,
}

impl WelcomeHero {
    pub fn new(
        user_name: Option<Option<String>>,
        summaries: Rc<[RepositorySummary]>,
        open_error: Option<QueryError>,
        on_open_repository: impl Fn(&mut Window, &mut App) + 'static,
        on_dismiss_open_error: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        WelcomeHero {
            user_name,
            summaries,
            open_error,
            on_open_repository: Rc::new(on_open_repository),
            on_dismiss_open_error: Rc::new(on_dismiss_open_error),
        }
    }
}

impl RenderOnce for WelcomeHero {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let greeting = match self.user_name.flatten() {
            Some(name) => format!(
                "Welcome back, {}.",
                name.split_whitespace().next().unwrap_or(&name)
            ),
            None => "Welcome back.".to_owned(),
        };
        let _ = (
            &self.open_error,
            &self.on_open_repository,
            &self.on_dismiss_open_error,
        );
        Placeholder::new("Welcome hero", "components/home/welcome-hero.tsx")
            .detail(format!("{greeting} {} repositories", self.summaries.len()))
    }
}
