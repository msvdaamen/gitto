//! **Stub.** The selected row's details. Replaces
//! `components/repository/details/commit-details.tsx`.

use gitto_git::features::diff::schema::ChangedFile;
use gitto_git::input::RepositoryId;
use gpui_kit::prelude::*;
use gpui_kit::{Context, EventEmitter, Window, div};

use crate::components::placeholder::Placeholder;
use crate::git::diff_source::DiffSource;
use crate::theme::PaletteExt as _;

/// What the details ask of the page (TS's `FileOpener`).
#[derive(Debug, Clone, PartialEq)]
pub enum CommitDetailsEvent {
    /// Shows the changes in a file of one of the lists, in place of the history.
    OpenFile {
        source: DiffSource,
        file: ChangedFile,
    },
    /// Loads a file's changes ahead, as it's likely to be opened (`prefetchFileDiff`).
    PrefetchFile {
        source: DiffSource,
        file: ChangedFile,
        uncounted: bool,
    },
}

/// Details of the selected history row: a commit, a stash, or the uncommitted changes (with the
/// commit form). Loads what it shows by itself, so it only needs the row's id from the history.
/// An entity: the page tells it the row ([`set_selected`](Self::set_selected)) and the file on
/// show, for its lists to mark ([`set_shown_file`](Self::set_shown_file)), and handles its
/// [`CommitDetailsEvent`]s.
///
/// Stub: renders a placeholder.
pub struct CommitDetails {
    repository_id: RepositoryId,
    selected_id: Option<String>,
    shown_file: Option<(DiffSource, String)>,
}

impl EventEmitter<CommitDetailsEvent> for CommitDetails {}

impl CommitDetails {
    pub fn new(repository_id: RepositoryId, _: &mut Window, _: &mut Context<Self>) -> Self {
        CommitDetails {
            repository_id,
            selected_id: None,
            shown_file: None,
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

    /// The row the details are of; `None` shows none.
    pub fn set_selected(&mut self, selected_id: Option<String>, cx: &mut Context<Self>) {
        if self.selected_id != selected_id {
            self.selected_id = selected_id;
            cx.notify();
        }
    }

    /// The file whose changes are on show, if any, by its source and path.
    pub fn set_shown_file(&mut self, shown: Option<(DiffSource, String)>, cx: &mut Context<Self>) {
        if self.shown_file != shown {
            self.shown_file = shown;
            cx.notify();
        }
    }
}

impl Render for CommitDetails {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().bg(cx.palette().panel).p_2().child(
            Placeholder::new("Commit details", "details/commit-details.tsx")
                .detail(format!("selected: {:?}", self.selected_id)),
        )
    }
}
