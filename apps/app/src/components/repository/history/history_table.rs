//! **Stub.** The repository's history. Replaces
//! `components/repository/history/history-table.tsx`.

use gitto_git::input::RepositoryId;
use gpui_kit::prelude::*;
use gpui_kit::{Context, EventEmitter, SharedString, Window, div};

use crate::components::placeholder::Placeholder;
use crate::git::rows::WIP_ID;
use crate::theme::PaletteExt as _;

/// How many rows from the end of the history loaded so far the rows in view come before more is
/// loaded: a few screenfuls, so it's usually there before it's scrolled to.
pub const LOAD_AHEAD: usize = 100;

/// What the history asks of the page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryEvent {
    /// A row was selected, by its id: the uncommitted changes' (`git::rows::WIP_ID`), a stash's
    /// or a commit's.
    Select(String),
}

/// The repository's history: the uncommitted changes, the stashes and the commits, with their
/// graph, filtered by the toolbar's search, loaded a page at a time as it's scrolled, moved
/// through with the arrow keys. An entity: it loads the log and keeps its scroll position. The
/// page tells it the search and the selection, and handles its [`HistoryEvent`]s.
///
/// Stub: renders a placeholder, and a button selecting the uncommitted changes.
pub struct HistoryTable {
    repository_id: RepositoryId,
    search: SharedString,
    selected_id: Option<String>,
    /// The row the details show: the selected one, once the selection stops moving quickly.
    details_id: Option<String>,
}

impl EventEmitter<HistoryEvent> for HistoryTable {}

impl HistoryTable {
    pub fn new(repository_id: RepositoryId, _: &mut Window, _: &mut Context<Self>) -> Self {
        HistoryTable {
            repository_id,
            search: SharedString::default(),
            selected_id: None,
            details_id: None,
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

    pub fn set_search(&mut self, search: SharedString, cx: &mut Context<Self>) {
        if self.search != search {
            self.search = search;
            cx.notify();
        }
    }

    pub fn set_selection(
        &mut self,
        selected_id: Option<String>,
        details_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if (&self.selected_id, &self.details_id) != (&selected_id, &details_id) {
            self.selected_id = selected_id;
            self.details_id = details_id;
            cx.notify();
        }
    }
}

impl Render for HistoryTable {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.selected_id.clone().unwrap_or_default();
        div()
            .id("history")
            .size_full()
            .bg(cx.palette().bg)
            .p_2()
            .on_click(cx.listener(|_, _, _, cx| cx.emit(HistoryEvent::Select(WIP_ID.to_owned()))))
            .child(
                Placeholder::new("History table", "history/history-table.tsx").detail(format!(
                    "search: {:?}, selected: {selected:?}; click to select the uncommitted changes",
                    self.search
                )),
            )
    }
}
