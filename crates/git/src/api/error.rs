//! Errors as the UI gets them. Ported from `toApiError` in `packages/git/src/core/middleware.ts`,
//! oRPC's handling of the errors it didn't make, and `isBug` in `packages/rpc/src/server.ts`.

use std::fmt;

use crate::core::errors::{ApiCode, Error};

/// An error as the UI shows it: its [`code`](Self::code), a [`message`](Self::message) for the
/// user, and whether it's a bug, to report rather than only tell the user ([`is_bug`]). Made from
/// an [`Error`], which it keeps for logs and bug reports ([`error`](Self::error)), by
/// `GitApi::fail` (or `GitApi::call`), which logs it and reports it if it's a bug: there's no
/// other way to make one, so none goes unreported.
///
/// The message is the error's own, but for errors the Electron app never told the renderer about:
/// - `Error::Bug` and `Error::Io` (TS's plain errors, and Node's), which say "Internal server
///   error", as oRPC put it.
/// - `Error::InvalidInput` (zod refusing input), which says "Input validation failed", as oRPC
///   put it. The UI checks input as it parses it (see `crate::input`), and can show that error's
///   own message there.
///
/// [`is_bug`]: Self::is_bug
#[derive(Debug, Clone)]
pub struct ApiError {
    code: ApiCode,
    message: String,
    /// Boxed, as `Error` is large, and this is what every call fails with.
    error: Box<Error>,
}

impl ApiError {
    /// `error` as the UI shows it, neither logged nor reported: only `GitApi::fail` makes one.
    pub(crate) fn new(error: Error) -> Self {
        let (code, message) = match &error {
            // Not TS's `ApiFailure`s: oRPC made them an INTERNAL_SERVER_ERROR without their message.
            Error::Bug(_) | Error::Io(_) => (
                ApiCode::InternalServerError,
                "Internal server error".to_owned(),
            ),
            // Refused by zod, before the procedure: oRPC's BAD_REQUEST.
            Error::InvalidInput(_) => (ApiCode::BadRequest, "Input validation failed".to_owned()),
            // `RepositoryListError` was an INTERNAL_SERVER_ERROR with its message, as the
            // repository router made it; the others are as they say.
            error => (error.api(), error.to_string()),
        };
        ApiError {
            code,
            message,
            error: Box::new(error),
        }
    }

    /// The kind of failure, e.g. `ApiCode::Conflict`; `code().code()` is the renderer's
    /// `CONFLICT`.
    pub fn code(&self) -> ApiCode {
        self.code
    }

    /// What the user is told.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The HTTP status it had in the Electron app, e.g. 409 for `PUSH_REWRITTEN`.
    pub fn status(&self) -> u16 {
        self.code.status()
    }

    /// Whether it's a bug, to report (see `ApiCode::is_bug`): git failing in a way Gitto doesn't
    /// expect, Gitto's own bug, a list of repositories that couldn't be read, or input the UI
    /// shouldn't have sent. Not one that's how things are, like a conflict.
    pub fn is_bug(&self) -> bool {
        self.code.is_bug()
    }

    /// The error itself, for logs and bug reports.
    pub fn error(&self) -> &Error {
        &self.error
    }

    pub fn into_error(self) -> Error {
        *self.error
    }

    /// `error` as the UI shows it, but neither logged nor reported, as `GitApi::fail` would: for
    /// fakes in tests, which have no `GitApi`.
    #[cfg(feature = "test-support")]
    pub fn unreported(error: Error) -> Self {
        ApiError::new(error)
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ApiError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&*self.error)
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::Arc;

    use super::*;
    use crate::core::errors::{GitError, GitErrorKind, ListAction, RepositoryListError};

    /// What the UI gets of `error`: the code as the renderer knew it, the message, and whether
    /// it's a bug.
    fn shown(error: Error) -> (&'static str, String, bool) {
        let error = ApiError::new(error);
        (
            error.code().code(),
            error.message().to_owned(),
            error.is_bug(),
        )
    }

    fn git_error(kind: GitErrorKind, message: &str) -> Error {
        Error::Git(GitError {
            kind,
            ..GitError::new(message, vec!["status".into()], Some(128), message)
        })
    }

    #[test]
    fn says_what_went_wrong_with_the_errors_code() {
        assert_eq!(
            shown(Error::RepositoryNotFound {
                repository_id: crate::input::RepositoryId::generate()
            }),
            ("NOT_FOUND", "Repository not found.".into(), false)
        );
        assert_eq!(
            shown(Error::NoUpstream(
                "main doesn't track a remote branch.".into()
            )),
            (
                "PRECONDITION_FAILED",
                "main doesn't track a remote branch.".into(),
                false
            )
        );
        assert_eq!(
            shown(Error::UnsupportedGit {
                version: Some("2.39.3".into()),
                required: "2.41".into()
            }),
            (
                "PRECONDITION_FAILED",
                "Gitto needs Git 2.41 or newer, but Git 2.39.3 is installed.".into(),
                false
            )
        );
        assert_eq!(
            shown(git_error(GitErrorKind::IndexLocked, "Another git process.")),
            ("CONFLICT", "Another git process.".into(), false)
        );
        assert_eq!(
            shown(git_error(
                GitErrorKind::NotARepository,
                "/r is no longer one."
            )),
            ("NOT_FOUND", "/r is no longer one.".into(), false)
        );
        assert_eq!(
            shown(Error::FileTooLarge { bytes: 1024 * 1024 }),
            (
                "PAYLOAD_TOO_LARGE",
                "This file is 1.0 MB, too large to show more of it.".into(),
                false
            )
        );
        assert_eq!(
            shown(Error::OutsideRepository {
                path: "../x".into()
            }),
            (
                "FORBIDDEN",
                "../x isn't a file in this repository.".into(),
                false
            )
        );
        assert_eq!(
            shown(Error::NotUtf8),
            (
                "UNSUPPORTED_MEDIA_TYPE",
                "This file isn't UTF-8 text.".into(),
                false
            )
        );
        assert_eq!(
            shown(Error::LinesNotStageable("Binary.".into())),
            ("UNPROCESSABLE_CONTENT", "Binary.".into(), false)
        );
    }

    #[test]
    fn gives_a_push_it_can_force_a_code_of_its_own() {
        let rewritten = ApiError::new(Error::PushRejected {
            message: "Rewritten.".into(),
            forcible: true,
        });
        assert_eq!(rewritten.code().code(), "PUSH_REWRITTEN");
        assert_eq!(rewritten.status(), 409);
        assert!(!rewritten.is_bug());
        let behind = ApiError::new(Error::PushRejected {
            message: "Behind.".into(),
            forcible: false,
        });
        assert_eq!(behind.code().code(), "PUSH_BEHIND");
        assert_eq!(behind.message(), "Behind.");
        assert!(!behind.is_bug());
    }

    #[test]
    fn reports_gits_unexpected_failures_as_bugs_saying_what_git_said() {
        assert_eq!(
            shown(git_error(GitErrorKind::Command, "fatal: bad object 0123")),
            (
                "INTERNAL_SERVER_ERROR",
                "fatal: bad object 0123".into(),
                true
            )
        );
    }

    #[test]
    fn hides_what_gittos_own_bugs_say_from_the_user() {
        let bug = ApiError::new(Error::Bug("Unexpected output: x".into()));
        assert_eq!(
            (bug.code().code(), bug.message(), bug.is_bug(), bug.status()),
            ("INTERNAL_SERVER_ERROR", "Internal server error", true, 500)
        );
        // Kept for the logs.
        assert_eq!(bug.error().to_string(), "Unexpected output: x");
        let io = Error::Io(Arc::new(io::Error::other("EACCES")));
        assert_eq!(
            shown(io),
            (
                "INTERNAL_SERVER_ERROR",
                "Internal server error".into(),
                true
            )
        );
    }

    #[test]
    fn reports_input_the_ui_shouldnt_have_sent_as_its_bug() {
        let error = ApiError::new(Error::InvalidInput("Not a branch. (\"x\")".into()));
        assert_eq!(
            (error.code().code(), error.message(), error.is_bug()),
            ("BAD_REQUEST", "Input validation failed", true)
        );
        assert_eq!(error.status(), 400);
        assert_eq!(error.error().to_string(), "Not a branch. (\"x\")");
    }

    #[test]
    fn says_why_the_list_of_repositories_couldnt_be_read_as_a_bug() {
        let locked = io::Error::other("database is locked");
        let error = Error::from(RepositoryListError::new(ListAction::Read, &locked));
        assert_eq!(
            shown(error),
            (
                "INTERNAL_SERVER_ERROR",
                "Gitto couldn't read its list of repositories: database is locked.".into(),
                true
            )
        );
    }

    #[test]
    fn is_an_error_with_the_message_and_the_original_as_its_source() {
        use std::error::Error as _;

        let error = ApiError::new(Error::HeadMoved);
        assert_eq!(
            error.to_string(),
            "The last commit changed before it could be amended, so nothing was amended."
        );
        assert_eq!(error.code(), ApiCode::Conflict);
        assert!(error.source().is_some());
        assert!(matches!(error.into_error(), Error::HeadMoved));
    }
}
