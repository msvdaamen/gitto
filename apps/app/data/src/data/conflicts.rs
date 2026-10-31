//! Resolving a conflicted file whole. Ported from `git/queries/conflicts.ts`.

use gitto_git::ContentVersion;
use gitto_git::features::conflicts::schema::{ConflictSides, FileVersion, Side};
use gitto_git::input::RepositoryId;
use gpui_kit::{App, Entity};

use super::mutation::Mutation;
use super::repository::Repositories;

/// Resolving a conflicted file whole: keeping one side, or marking it resolved as it is.
#[derive(Debug, Clone, PartialEq)]
pub enum ConflictResolution {
    Keep {
        side: Side,
        /// The sides the user saw, which have to be the file's still.
        sides: ConflictSides,
        /// The version of the file the user saw (`ConflictFile::version`).
        version: Option<FileVersion>,
    },
    Mark {
        version: Option<ContentVersion>,
        /// Even with conflict markers left: the user said they belong in it.
        with_markers: bool,
    },
}

/// A conflicted file to resolve whole, and how.
#[derive(Debug, Clone, PartialEq)]
pub struct FileToResolve {
    pub repository_id: RepositoryId,
    pub path: String,
    pub resolution: ConflictResolution,
}

/// Resolves a conflicted file whole (see [`ConflictResolution`]; `useResolveFile`). Settles once
/// that's done, rather than once the uncommitted changes are reloaded after, so the view can move
/// on at once.
pub fn resolve_file_mutation(cx: &mut App) -> Entity<Mutation<FileToResolve>> {
    Mutation::new(
        |file: FileToResolve, cx: &mut App| {
            let FileToResolve {
                repository_id: id,
                path,
                resolution,
            } = file;
            Repositories::update_opened(
                &id,
                |repository, cx| repository.resolve_file(path, resolution, cx),
                cx,
            )
        },
        cx,
    )
}
