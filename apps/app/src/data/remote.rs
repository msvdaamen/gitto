//! Fetching, and why a push was turned down. Ported from
//! `git/queries/remote.ts`.

use gitto_git::ApiCode;
use gitto_git::input::RepositoryId;
use gpui_kit::{App, Entity};

use super::Repositories;
use super::error::DataError;
use super::mutation::Mutation;

/// Fetches every remote of the repository whose id it's given (`useFetch`). The id is the
/// mutation's input (`Mutation::input`), so a fetch still running after switching repositories
/// reloads the one it fetched, and its state can be told apart from the current repository's.
pub fn fetch_mutation(cx: &mut App) -> Entity<Mutation<RepositoryId>> {
    Mutation::new(
        |id: RepositoryId, cx: &mut App| {
            Repositories::open(&id, cx).update(cx, |repository, cx| repository.fetch(cx))
        },
        cx,
    )
}

/// Why the remote branch turned a push down (see [`push_rejection`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushRejection {
    /// Its commits are ones the branch had before a rebase or an amend, which force-pushing
    /// replaces.
    Rewritten,
    /// Its commits are others, to pull.
    Behind,
}

/// Why the remote branch turned a push down, if `error` says it has commits the branch doesn't
/// (`pushRejection`).
pub fn push_rejection(error: Option<&DataError>) -> Option<PushRejection> {
    let error = error?;
    if error.has_code(ApiCode::PushRewritten) {
        Some(PushRejection::Rewritten)
    } else if error.has_code(ApiCode::PushBehind) {
        Some(PushRejection::Behind)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tells_why_a_push_was_turned_down() {
        let rejected = |forcible: bool| {
            DataError::from(gitto_git::Error::PushRejected {
                message: "Rejected.".to_owned(),
                forcible,
            })
        };
        assert_eq!(
            push_rejection(Some(&rejected(true))),
            Some(PushRejection::Rewritten)
        );
        assert_eq!(
            push_rejection(Some(&rejected(false))),
            Some(PushRejection::Behind)
        );
        assert_eq!(push_rejection(Some(&DataError::msg("Offline."))), None);
        assert_eq!(push_rejection(None), None);
    }
}
