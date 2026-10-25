//! Staging, unstaging and discarding changes: whole files, or lines of them. Ported from
//! `git/queries/staging.ts`.

use gitto_git::features::diff::schema::ChangedFile;
use gitto_git::features::staging::discard::DiscardSide;
use gitto_git::features::staging::schema::LineSelection;
use gitto_git::input::RepositoryId;
use gpui_kit::{App, Entity};

use super::mutation::Mutation;
use super::repository::Repositories;
use gitto_ui::git::changes::staging_paths;

/// Files to stage or unstage: their paths (see `gitto_ui::git::changes::staging_paths`), or all
/// of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StagingTarget {
    Paths(Vec<String>),
    All,
}

/// Stages files in the repository it's given (`useStage`).
pub fn stage_mutation(cx: &mut App) -> Entity<Mutation<(RepositoryId, StagingTarget)>> {
    Mutation::new(
        |(id, target): (RepositoryId, StagingTarget), cx: &mut App| {
            Repositories::open(&id, cx).update(cx, |repository, cx| repository.stage(target, cx))
        },
        cx,
    )
}

/// Unstages files in the repository it's given (`useUnstage`).
pub fn unstage_mutation(cx: &mut App) -> Entity<Mutation<(RepositoryId, StagingTarget)>> {
    Mutation::new(
        |(id, target): (RepositoryId, StagingTarget), cx: &mut App| {
            Repositories::open(&id, cx).update(cx, |repository, cx| repository.unstage(target, cx))
        },
        cx,
    )
}

/// Changes to discard: a file's on one side of the uncommitted changes, or all of them.
#[derive(Debug, Clone, PartialEq)]
pub enum DiscardTarget {
    File {
        side: DiscardSide,
        file: ChangedFile,
    },
    All,
}

/// Discards changes, which are lost: a file's unstaged ones, or staged ones and all, by both its
/// paths if it's renamed; or every change, deleting untracked files too (`useDiscard`). A file
/// that changed since its list was read isn't discarded (see `discard`).
pub fn discard_mutation(cx: &mut App) -> Entity<Mutation<(RepositoryId, DiscardTarget)>> {
    Mutation::new(
        |(id, target): (RepositoryId, DiscardTarget), cx: &mut App| {
            Repositories::open(&id, cx).update(cx, |repository, cx| repository.discard(target, cx))
        },
        cx,
    )
}

/// Whether to stage or unstage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageAction {
    Stage,
    Unstage,
}

/// A file to stage or unstage whole.
#[derive(Debug, Clone, PartialEq)]
pub struct FileToStage {
    pub repository_id: RepositoryId,
    pub action: StageAction,
    pub file: ChangedFile,
}

/// Stages or unstages a whole file: a renamed one by both its paths (`useStageFile`). Settles
/// once that's done, rather than once the uncommitted changes are reloaded after, so the view can
/// move on at once.
pub fn stage_file_mutation(cx: &mut App) -> Entity<Mutation<FileToStage>> {
    Mutation::new(
        |input: FileToStage, cx: &mut App| {
            let FileToStage {
                repository_id: id,
                action,
                file,
            } = input;
            let paths = staging_paths(std::slice::from_ref(&file));
            Repositories::open(&id, cx).update(cx, |repository, cx| {
                repository.stage_file(action, paths, cx)
            })
        },
        cx,
    )
}

/// Lines of a file's changes to stage or unstage, picked from its patch on show, named by its
/// `version` (see `patch_version`).
#[derive(Debug, Clone, PartialEq)]
pub struct LinesToStage {
    pub repository_id: RepositoryId,
    pub action: StageAction,
    pub file: ChangedFile,
    pub version: String,
    pub lines: LineSelection,
}

/// Stages or unstages lines of a file's changes; resolves to the file's patch they leave
/// (`useStageLines`). That takes the place of the one on show at once, if it has changes left: one
/// without is left for the view to move on from, rather than said to have none (see `has_hunks`).
/// The rest of the uncommitted changes, like the lists, are reloaded without waiting for them. If
/// the lines couldn't be staged, it settles once all of it is reloaded: the patch they were picked
/// from may be out of date, and isn't picked from again until it isn't.
pub fn stage_lines_mutation(cx: &mut App) -> Entity<Mutation<LinesToStage, String>> {
    Mutation::new(
        |input: LinesToStage, cx: &mut App| {
            let LinesToStage {
                repository_id: id,
                action,
                file,
                version,
                lines,
            } = input;
            Repositories::open(&id, cx).update(cx, |repository, cx| {
                repository.stage_lines(action, file, version, lines, cx)
            })
        },
        cx,
    )
}
