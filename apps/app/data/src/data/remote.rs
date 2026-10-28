//! Why a push was turned down. Ported from `git/queries/remote.ts`; fetching is a repository's
//! change (`Repository::fetch`).

use gitto_git::ApiCode;

use super::error::DataError;

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
