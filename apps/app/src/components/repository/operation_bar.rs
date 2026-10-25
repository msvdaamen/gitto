//! **Stub.** What's under way in the repository. Replaces
//! `components/repository/operation-bar.tsx`.

use gitto_git::features::diff::schema::ChangedFile;
use gitto_git::input::RepositoryId;
use gpui_kit::prelude::*;
use gpui_kit::{Context, EventEmitter, Window, div};

use super::BeforeChange;

/// What the operation bar asks of the page.
#[derive(Debug, Clone, PartialEq)]
pub enum OperationBarEvent {
    /// Opens a conflicted file's conflicts.
    Resolve(ChangedFile),
}

/// What's under way in the repository, above the history and a file's changes: a merge, a
/// rebase, cherry-picks or reverts, with how many files still have conflicts, and the conflicts a
/// stash left as it was popped. The first conflicted file is a click away; once none is left, the
/// operation is continued, as git would from a terminal. One that goes on to stop at the next
/// commit's conflicts, as a rebase of several commits can, opens the first of those to resolve.
/// Aborting it, which throws away what's been resolved, asks first.
///
/// An entity: it loads the operation and the conflicted files. Continuing or aborting waits for
/// [`set_before_change`](Self::set_before_change)'s guard, as either rewrites the working tree.
///
/// Stub: renders nothing, as there's nothing under way until it's built.
pub struct OperationBar {
    repository_id: RepositoryId,
    before_change: Option<BeforeChange>,
}

impl EventEmitter<OperationBarEvent> for OperationBar {}

impl OperationBar {
    pub fn new(repository_id: RepositoryId, _: &mut Window, _: &mut Context<Self>) -> Self {
        OperationBar {
            repository_id,
            before_change: None,
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

    /// What continuing or aborting waits for: the edits to a file on show saved.
    pub fn set_before_change(&mut self, guard: BeforeChange) {
        self.before_change = Some(guard);
    }
}

impl Render for OperationBar {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let _ = &self.before_change;
        div()
    }
}
