//! Committing and rewording. Ported from `git/queries/commit.ts`.

use gitto_git::Result;
use gitto_git::input::{FullSha, Message, RepositoryId};
use gpui_kit::{App, Entity};

use super::mutation::Mutation;
use super::repository::Repositories;

/// A commit to make: of what's staged, with `message`; with `amend`, HEAD's SHA, it replaces that
/// commit instead.
#[derive(Debug, Clone, PartialEq)]
pub struct NewCommit {
    pub message: Message,
    pub amend: Option<FullSha>,
}

impl NewCommit {
    /// The commit with `message`, amending `amend`, if they're valid.
    pub fn parse(message: impl Into<String>, amend: Option<String>) -> Result<Self> {
        Ok(NewCommit {
            message: Message::parse(message)?,
            amend: amend.map(FullSha::parse).transpose()?,
        })
    }
}

/// A rewording of HEAD's commit, `sha`.
#[derive(Debug, Clone, PartialEq)]
pub struct Reword {
    pub sha: FullSha,
    pub message: Message,
}

impl Reword {
    /// The rewording of `sha` with `message`, if they're valid.
    pub fn parse(sha: impl Into<String>, message: impl Into<String>) -> Result<Self> {
        Ok(Reword {
            sha: FullSha::parse(sha)?,
            message: Message::parse(message)?,
        })
    }
}

/// Commits what's staged in the repository it's given (see [`NewCommit`]; `useCreateCommit`).
pub fn create_commit_mutation(cx: &mut App) -> Entity<Mutation<(RepositoryId, NewCommit)>> {
    Mutation::new(
        |(id, commit): (RepositoryId, NewCommit), cx: &mut App| {
            Repositories::open(&id, cx)
                .update(cx, |repository, cx| repository.create_commit(commit, cx))
        },
        cx,
    )
}

/// Rewrites the message of HEAD's commit, leaving what's staged out of it; resolves to the
/// rewritten commit's SHA once the repository has reloaded (`useRewordCommit`).
pub fn reword_commit_mutation(cx: &mut App) -> Entity<Mutation<(RepositoryId, Reword), String>> {
    Mutation::new(
        |(id, reword): (RepositoryId, Reword), cx: &mut App| {
            Repositories::open(&id, cx)
                .update(cx, |repository, cx| repository.reword_commit(reword, cx))
        },
        cx,
    )
}
