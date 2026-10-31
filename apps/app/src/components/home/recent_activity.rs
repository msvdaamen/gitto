//! **Stub.** What was last done in the repositories. Replaces
//! `components/home/recent-activity.tsx`.

use std::rc::Rc;

use gpui_kit::prelude::*;
use gpui_kit::{App, Window, div};

use super::summaries::RepositorySummary;
use crate::components::placeholder::Placeholder;

/// How many of the latest things done, across all repositories, are shown.
pub const SHOWN: usize = 6;

/// What was last done in any of the repositories, as their reflogs have it. Stub: renders a
/// placeholder.
#[derive(IntoElement)]
pub struct RecentActivity {
    summaries: Rc<[RepositorySummary]>,
}

impl RecentActivity {
    pub fn new(summaries: Rc<[RepositorySummary]>) -> Self {
        RecentActivity { summaries }
    }
}

impl RenderOnce for RecentActivity {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let entries: usize = self
            .summaries
            .iter()
            .filter_map(|summary| summary.overview.as_ref())
            .map(|overview| overview.activity.len())
            .sum();
        div().h_64().child(
            Placeholder::new("Recent activity", "components/home/recent-activity.tsx")
                .detail(format!("{} of {entries} entries", entries.min(SHOWN))),
        )
    }
}
