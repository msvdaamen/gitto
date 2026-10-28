//! Staging, unstaging and discarding changes: whole files, or lines of them. Ported from
//! `git/queries/staging.ts`.

use std::sync::Arc;

use futures::FutureExt as _;
use futures::future::BoxFuture;
use gitto_git::PatchVersion;
use gitto_git::features::diff::schema::ChangedFile;
use gitto_git::features::staging::commands::{
    stage, stage_all, stage_lines, unstage, unstage_all, unstage_lines,
};
use gitto_git::features::staging::discard::{DiscardSide, Discarded, discard, discard_all};
use gitto_git::features::staging::schema::LineSelection;
use gitto_git::input::{FileInput, Paths, RepoPath, RepositoryId};
use gitto_git::schema::FileStatus;
use gitto_git::{ApiError, GitApi};
use gpui_kit::{App, AppContext as _, Entity};

use super::call::{call, git, parse};
use super::client::QueryClient;
use super::file_diff::FilePatch;
use super::keys::{self, UncommittedSide};
use super::mutation::Mutation;
use crate::git::changes::staging_paths;
use crate::git::patch::has_hunks;

/// Files to stage or unstage: their paths (see `git::changes::staging_paths`), or all of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StagingTarget {
    Paths(Vec<String>),
    All,
}

/// Stages files in the repository it's given (`useStage`).
pub fn stage_mutation(cx: &mut App) -> Entity<Mutation<(RepositoryId, StagingTarget)>> {
    staging_mutation(
        |git, id, target| match target {
            StagingTarget::All => git
                .call("staging.stageAll", &id, |repo| async move {
                    stage_all(&repo).await
                })
                .boxed(),
            StagingTarget::Paths(paths) => git
                .call("staging.stage", &id, |repo| async move {
                    stage(&repo, &Paths::parse(paths)?).await
                })
                .boxed(),
        },
        cx,
    )
}

/// Unstages files in the repository it's given (`useUnstage`).
pub fn unstage_mutation(cx: &mut App) -> Entity<Mutation<(RepositoryId, StagingTarget)>> {
    staging_mutation(
        |git, id, target| match target {
            StagingTarget::All => git
                .call("staging.unstageAll", &id, |repo| async move {
                    unstage_all(&repo).await
                })
                .boxed(),
            StagingTarget::Paths(paths) => git
                .call("staging.unstage", &id, |repo| async move {
                    unstage(&repo, &Paths::parse(paths)?).await
                })
                .boxed(),
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
    staging_mutation(
        |git, id, target| match target {
            DiscardTarget::All => git
                .call("staging.discardAll", &id, |repo| async move {
                    discard_all(&repo).await
                })
                .boxed(),
            DiscardTarget::File { side, file } => git
                .call("staging.discard", &id, move |repo| async move {
                    let discarded = Discarded {
                        path: file.path,
                        orig_path: file.orig_path,
                        status: file.status,
                        side,
                    };
                    discard(&repo, &discarded).await
                })
                .boxed(),
        },
        cx,
    )
}

/// A mutation of the staging in the repository it's given, which `run` does; the uncommitted
/// changes reload once it's done, and it settles once they have (`useStagingMutation`).
fn staging_mutation<T: Clone + 'static>(
    run: impl Fn(GitApi, RepositoryId, T) -> BoxFuture<'static, Result<(), ApiError>> + 'static,
    cx: &mut App,
) -> Entity<Mutation<(RepositoryId, T)>> {
    Mutation::new(
        move |(id, target): (RepositoryId, T), cx: &mut App| {
            let action = run(git(cx), id.clone(), target);
            cx.spawn(async move |cx| {
                cx.background_spawn(action).await?;
                // The watcher would catch this too, but reloading right away feels snappier.
                cx.update(|cx| QueryClient::invalidate(&keys::uncommitted(&id), cx))
                    .await;
                Ok(())
            })
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
            let procedure = match action {
                StageAction::Stage => "staging.stage",
                StageAction::Unstage => "staging.unstage",
            };
            let staging = call(cx, procedure, &id, move |repo| async move {
                let paths = Paths::parse(paths)?;
                match action {
                    StageAction::Stage => stage(&repo, &paths).await,
                    StageAction::Unstage => unstage(&repo, &paths).await,
                }
            });
            cx.spawn(async move |cx| {
                let result = cx.background_spawn(staging).await;
                cx.update(|cx| {
                    let _ = QueryClient::invalidate(&keys::uncommitted(&id), cx);
                });
                result.map_err(Into::into)
            })
        },
        cx,
    )
}

/// Lines of a file's changes to stage or unstage, picked from its patch on show, named by its
/// `version` (see `PatchVersion`).
#[derive(Debug, Clone, PartialEq)]
pub struct LinesToStage {
    pub repository_id: RepositoryId,
    pub action: StageAction,
    pub file: ChangedFile,
    pub version: PatchVersion,
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
            let path = file.path.clone();
            let staging = {
                let procedure = match action {
                    StageAction::Stage => "staging.stageLines",
                    StageAction::Unstage => "staging.unstageLines",
                };
                call(cx, procedure, &id, move |repo| async move {
                    let input = FileInput {
                        path: parse::<RepoPath>(file.path)?,
                        orig_path: file.orig_path.map(parse::<RepoPath>).transpose()?,
                    };
                    match action {
                        StageAction::Stage => {
                            let untracked = file.status == FileStatus::Untracked;
                            stage_lines(&repo, &input, untracked, &version, &lines).await
                        }
                        StageAction::Unstage => {
                            unstage_lines(&repo, &input, &version, &lines).await
                        }
                    }
                })
            };
            cx.spawn(async move |cx| match cx.background_spawn(staging).await {
                Ok(patch) => {
                    cx.update(|cx| show_patch_left(&id, action, &path, &patch, cx));
                    Ok(patch)
                }
                Err(error) => {
                    cx.update(|cx| QueryClient::invalidate(&keys::uncommitted(&id), cx))
                        .await;
                    Err(error.into())
                }
            })
        },
        cx,
    )
}

/// Shows `patch`, what staging or unstaging lines of the file at `path` left of its changes, in
/// place of the patch they were picked from, if it has changes left; and reloads the rest of the
/// uncommitted changes, without waiting for them.
fn show_patch_left(id: &RepositoryId, action: StageAction, path: &str, patch: &str, cx: &mut App) {
    let side = match action {
        StageAction::Stage => UncommittedSide::Unstaged,
        StageAction::Unstage => UncommittedSide::Staged,
    };
    let key = keys::uncommitted_file_patch(id, side, path);
    // A reload under way, e.g. the watcher's, could be of the patch from before.
    QueryClient::cancel(&key, cx);
    let shown = has_hunks(patch)
        && QueryClient::update_data::<FilePatch>(
            &key,
            |last| FilePatch {
                patch: Arc::from(patch),
                ..last.clone()
            },
            cx,
        );
    let _ = QueryClient::invalidate_where(
        &keys::uncommitted(id),
        |entry| !shown || entry.key != key,
        cx,
    );
}
