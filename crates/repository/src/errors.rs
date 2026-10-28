//! Errors the repository crate returns. Ported from `packages/repository/src/server/errors.ts`,
//! where each is a class; here each is a variant of [`Error`]. TS's handler (`toApiError`) told the
//! renderer a [`Error::NotAGitRepository`] as a bad request and a [`Error::List`] as an internal
//! error: [`Error::is_bug`] says which.

use std::fmt;
use std::sync::Arc;

use gitto_db::rusqlite;
use gitto_git::RepositoryListError;

/// What was done with the list of repositories when it failed: gitto-git's, whose
/// `RepositoryListError` is what [`ListError`] says.
pub use gitto_git::ListAction;

/// What the repository crate's functions return.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Why a repository couldn't be added, or the list of them read or changed.
#[derive(Debug, Clone, thiserror::Error)]
pub enum Error {
    /// The folder someone tried to add isn't a git repository.
    #[error("{path} is not a git repository.")]
    NotAGitRepository { path: String },
    /// The folder someone tried to add has a path that isn't UTF-8, which Gitto can't keep in its
    /// list: `path` is it, lossily.
    #[error("Gitto can't add {path}, as its path isn't UTF-8.")]
    PathNotUtf8 { path: String },
    /// Gitto's list of repositories couldn't be read or changed.
    #[error(transparent)]
    List(#[from] ListError),
}

impl Error {
    /// A failure to `action` the list, because of `cause`.
    pub fn list(action: ListAction, cause: rusqlite::Error) -> Error {
        Error::List(ListError::new(action, cause))
    }

    /// Whether this is a bug to report rather than something to tell the user: a list that can't
    /// be read or changed is (TS's `INTERNAL_SERVER_ERROR`), a folder that can't be added isn't
    /// (`BAD_REQUEST`).
    pub fn is_bug(&self) -> bool {
        matches!(self, Error::List(_))
    }
}

/// Gitto's list of repositories couldn't be read or changed, e.g. as its database is locked or the
/// disk is full: said as gitto-git's `RepositoryListError` says it, with SQLite's reason, which
/// is its source.
///
/// The reason is the [`rusqlite::Error`]'s message, which is SQLite's (in TS, the error
/// node:sqlite threw, which Drizzle's own wrapped in its query). Its source is only SQLite's
/// result code, said in general terms (`Error code 19: A constraint violation occurred` for
/// `UNIQUE constraint failed: repositories.path`), so it isn't the reason.
#[derive(Debug, Clone)]
pub struct ListError {
    list: RepositoryListError,
    cause: Arc<rusqlite::Error>,
}

impl fmt::Display for ListError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.list.fmt(f)
    }
}

/// By hand, as thiserror's `#[source]` would be the `Arc`, not the `rusqlite::Error` in it.
impl std::error::Error for ListError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.cause.as_ref())
    }
}

impl ListError {
    /// A failure to `action` the list, because of `cause`.
    pub fn new(action: ListAction, cause: rusqlite::Error) -> Self {
        ListError {
            list: RepositoryListError::with_reason(action, &cause.to_string()),
            cause: Arc::new(cause),
        }
    }

    pub fn action(&self) -> ListAction {
        self.list.action
    }

    /// The error as gitto-git has it, for a repository it couldn't look up.
    pub fn as_repository_list_error(&self) -> &RepositoryListError {
        &self.list
    }

    pub fn cause(&self) -> &rusqlite::Error {
        &self.cause
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sqlite_error(code: i32, message: Option<&str>) -> rusqlite::Error {
        rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(code), message.map(str::to_owned))
    }

    #[test]
    fn says_a_folder_is_not_a_git_repository() {
        let error = Error::NotAGitRepository {
            path: "/home/me/notes".to_owned(),
        };
        assert_eq!(error.to_string(), "/home/me/notes is not a git repository.");
        assert!(!error.is_bug());
    }

    #[test]
    fn says_why_the_list_could_not_be_read_or_updated() {
        let locked = || sqlite_error(5, Some("database is locked"));
        let read = Error::list(ListAction::Read, locked());
        assert_eq!(
            read.to_string(),
            "Gitto couldn't read its list of repositories: database is locked."
        );
        assert!(read.is_bug());
        let update = Error::list(ListAction::Update, locked());
        assert_eq!(
            update.to_string(),
            "Gitto couldn't update its list of repositories: database is locked."
        );
    }

    #[test]
    fn keeps_the_cause() {
        let error = Error::list(
            ListAction::Read,
            sqlite_error(13, Some("database or disk is full")),
        );
        let source = std::error::Error::source(&error).unwrap();
        assert!(source.is::<rusqlite::Error>());
        assert_eq!(source.to_string(), "database or disk is full");
    }

    #[test]
    fn says_it_as_gitto_git_does() {
        let error = ListError::new(
            ListAction::Update,
            sqlite_error(5, Some("database is locked")),
        );
        let list = error.as_repository_list_error();
        assert_eq!(list.action, ListAction::Update);
        assert_eq!(list.to_string(), error.to_string());
    }

    #[test]
    fn takes_sqlites_message_not_its_result_code() {
        let unique = sqlite_error(19, Some("UNIQUE constraint failed: repositories.path"));
        // Its source, the result code, says it in general terms.
        let code = std::error::Error::source(&unique).unwrap().to_string();
        assert!(code.starts_with("Error code 19: "), "{code}");
        assert_eq!(
            Error::list(ListAction::Update, unique).to_string(),
            "Gitto couldn't update its list of repositories: UNIQUE constraint failed: \
             repositories.path."
        );
        // Without a message from SQLite, the result code's is all there is.
        assert_eq!(
            Error::list(ListAction::Read, sqlite_error(5, None)).to_string(),
            "Gitto couldn't read its list of repositories: Error code 5: database is locked."
        );
    }

    #[test]
    fn trims_the_message_and_keeps_its_own_punctuation() {
        let reason = |message| {
            Error::list(ListAction::Read, sqlite_error(1, Some(message)))
                .to_string()
                .trim_start_matches("Gitto couldn't read its list of repositories: ")
                .to_owned()
        };
        assert_eq!(
            reason("  no such table: repositories \n"),
            "no such table: repositories."
        );
        assert_eq!(reason("Done."), "Done.");
        assert_eq!(reason("Really!"), "Really!");
        assert_eq!(reason("Why?"), "Why?");
    }
}
