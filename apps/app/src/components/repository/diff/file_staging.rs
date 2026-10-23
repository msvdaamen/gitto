//! Staging and unstaging an uncommitted file from its diff: whole, or its lines a hunk or a
//! selection at a time (see `PatchViewer`); one at a time. Once that leaves it without changes on
//! its side, the next file in its list opens. Ported from `file-staging.tsx`.

use std::rc::Rc;
use std::sync::Arc;

use gitto_git::features::diff::schema::ChangedFile;
use gitto_git::features::staging::patch_version::patch_version;
use gitto_git::features::staging::schema::LineSelection;
use gitto_ui::git::changes::is_nested_repository;
use gitto_ui::git::patch::{has_hunks, staged_whole};
use gpui_kit::{App, Context, Entity, Subscription, Task};

use super::file_diff_view::{FileDiffEvent, FileDiffView};
use super::line_picker::{LineStaging, StageLines};
use crate::data::staging::{
    FileToStage, LinesToStage, StageAction, stage_file_mutation, stage_lines_mutation,
};
use crate::data::{Mutation, QueryError};
use crate::git::diff_source::DiffSource;

/// Stages, or unstages, a file's lines; resolves to the patch they leave.
pub type StageLinesMutation = Entity<Mutation<LinesToStage, String>>;
/// Stages, or unstages, a whole file.
pub type StageFileMutation = Entity<Mutation<FileToStage>>;

/// The view's staging (`useFileStaging`): what it runs, and how it's going.
pub struct FileStaging {
    stage_lines: StageLinesMutation,
    stage_file: StageFileMutation,
    /// The whole file is being staged.
    staging_whole: bool,
    /// The patch lines were last staged from: none are again while it's on show and another's
    /// in.
    staged_from: Option<Arc<str>>,
    /// Why the last lines, or the whole file, couldn't be staged, and of which file (its key).
    error: Option<(String, String)>,
    /// What the viewer stages lines with, made once, so it's the same staging each time.
    stage: StageLines,
    _subscriptions: [Subscription; 2],
}

impl FileStaging {
    /// The view's staging, with git.
    pub fn new(cx: &mut Context<FileDiffView>) -> Self {
        let stage_lines = stage_lines_mutation(cx);
        let stage_file = stage_file_mutation(cx);
        FileStaging::with(stage_lines, stage_file, cx)
    }

    /// The view's staging, with the mutations given: a test's.
    pub fn with(
        stage_lines: StageLinesMutation,
        stage_file: StageFileMutation,
        cx: &mut Context<FileDiffView>,
    ) -> Self {
        let view = cx.entity().downgrade();
        let stage: StageLines = Rc::new(move |from, lines, cx: &mut App| {
            view.update(cx, |view, cx| view.stage_lines(from, lines, cx))
                .unwrap_or_else(|_| Task::ready(Err(QueryError::msg("The file was closed."))))
        });
        let subscriptions = [
            cx.observe(&stage_lines, |_, _, cx| cx.notify()),
            cx.observe(&stage_file, |_, _, cx| cx.notify()),
        ];
        FileStaging {
            stage_lines,
            stage_file,
            staging_whole: false,
            staged_from: None,
            error: None,
            stage,
            _subscriptions: subscriptions,
        }
    }

    /// Lines, or the whole file, are being staged: nothing else is until they are.
    pub fn is_staging(&self, cx: &App) -> bool {
        self.stage_lines.read(cx).is_pending() || self.staging_whole
    }

    /// Why the last lines, or the whole file, couldn't be staged, if it was the file `key`.
    pub fn error_for(&self, key: &str) -> Option<&str> {
        self.error
            .as_ref()
            .filter(|(failed, _)| failed == key)
            .map(|(_, message)| message.as_str())
    }
}

impl FileDiffView {
    /// What's done with the file's changes: staged, or unstaged, by the side they're on.
    pub(super) fn staging_action(&self) -> StageAction {
        if *self.view.source() == DiffSource::Staged {
            StageAction::Unstage
        } else {
            StageAction::Stage
        }
    }

    /// What the viewer takes to stage lines, if the file's can be: an uncommitted one's, on show,
    /// unless it can only be staged whole (a link's, say).
    pub(super) fn line_staging(&self, cx: &App) -> Option<LineStaging> {
        let data = self.view.current()?;
        if !self.view.source().is_uncommitted() || !self.view.shows_patch() {
            return None;
        }
        if staged_whole(&data.patch) {
            return None;
        }
        let shown = self.view.shown_patch().map(|shown| &shown.patch);
        let left =
            self.staging.staged_from.as_ref().is_some_and(|last| {
                shown.is_some_and(|shown| shown == last) && data.patch != *last
            });
        Some(LineStaging {
            action: self.staging_action(),
            busy: self.staging.is_staging(cx) || left,
            stage: Rc::clone(&self.staging.stage),
        })
    }

    /// Stages `lines` of `from`, the patch on show they were picked from; resolves once they're
    /// staged, and fails if they couldn't be. Once that leaves none, on to the next file.
    pub(super) fn stage_lines(
        &mut self,
        from: String,
        lines: LineSelection,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), QueryError>> {
        let key = self.view.file_key();
        self.staging.error = None;
        let input = LinesToStage {
            repository_id: self.view.repository_id().clone(),
            action: self.staging_action(),
            file: self.view.file(),
            version: patch_version(&from),
            lines,
        };
        let staged = self
            .staging
            .stage_lines
            .update(cx, |mutation, cx| mutation.mutate(input, cx));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let left = match staged.await {
                Ok(Ok(left)) => left,
                Ok(Err(error)) => {
                    let message = error.message();
                    let _ = this.update(cx, |this, cx| {
                        this.staging.error = Some((key, message));
                        cx.notify();
                    });
                    return Err(error);
                }
                Err(_) => return Err(QueryError::msg("The lines weren't staged.")),
            };
            let _ = this.update(cx, |this, cx| {
                // The patch staged from stays on show until the one they leave is highlighted;
                // once there are none left, so does this file's until the next one's is in.
                this.staging.staged_from = Some(from.into());
                if !has_hunks(&left) {
                    let target = this.view.target();
                    this.move_on(&key, target, cx);
                }
                cx.notify();
            });
            Ok(())
        })
    }

    /// Whether the file can be staged, or unstaged, whole, and which: one of the uncommitted
    /// changes in its list, but not a repository inside this one, which would be added as a
    /// submodule without its URL.
    pub(super) fn whole_file(&self) -> Option<StageAction> {
        (self.view.source().is_uncommitted()
            && self.view.listed()
            && !is_nested_repository(&self.view.file()))
        .then(|| self.staging_action())
    }

    /// Whether staging the whole file can't be asked for now: something's being staged, or
    /// another file's on show.
    pub(super) fn staging_disabled(&self, cx: &App) -> bool {
        self.staging.is_staging(cx) || self.view.shows_other()
    }

    /// Stages, or unstages, the whole file; then on to the next file, as once its last lines are.
    pub(super) fn stage_whole(&mut self, cx: &mut Context<Self>) {
        let Some(action) = self.whole_file() else {
            return;
        };
        if self.staging_disabled(cx) {
            return;
        }
        let key = self.view.file_key();
        // Picked while the file's still in its list: it's staged before the list is reloaded.
        let target = self.view.target();
        self.staging.staging_whole = true;
        self.staging.error = None;
        let input = FileToStage {
            repository_id: self.view.repository_id().clone(),
            action,
            file: self.view.live_file(),
        };
        let staged = self
            .staging
            .stage_file
            .update(cx, |mutation, cx| mutation.mutate(input, cx));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = staged.await;
            let _ = this.update(cx, |this, cx| {
                this.staging.staging_whole = false;
                match result {
                    Ok(Ok(())) => this.move_on(&key, target, cx),
                    Ok(Err(error)) => this.staging.error = Some((key, error.message())),
                    Err(_) => {}
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Once the file `key` names has no changes left on its side, on to `target`. Not if another
    /// file was opened meanwhile.
    fn move_on(&mut self, key: &str, target: Option<ChangedFile>, cx: &mut Context<Self>) {
        if let Some(file) = target
            && key == self.view.file_key()
        {
            cx.emit(FileDiffEvent::Open {
                source: self.view.source().clone(),
                file,
            });
        }
    }
}
