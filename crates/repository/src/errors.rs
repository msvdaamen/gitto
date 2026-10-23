//! Errors the repository crate returns. Ported from `packages/repository/src/server/errors.ts`,
//! where each is a class; here each is a variant of [`Error`]. TS's handler (`toApiError`) told the
//! renderer a [`Error::NotAGitRepository`] as a bad request and a [`Error::List`] as an internal
//! error: [`Error::is_bug`] says which.

use std::fmt;
use std::sync::Arc;

use gitto_db::rusqlite;

/// What the repository crate's functions return.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Why a repository couldn't be added, or the list of them read or changed.
#[derive(Debug, Clone)]
pub enum Error {
    /// The folder someone tried to add isn't a git repository.
    NotAGitRepository { path: String },
    /// Gitto's list of repositories couldn't be read or changed, e.g. as its database is locked or
    /// the disk is full: said with SQLite's reason (see [`reason`]). `cause` is its source.
    List {
        action: ListAction,
        cause: Arc<rusqlite::Error>,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotAGitRepository { path } => write!(f, "{path} is not a git repository."),
            Error::List { action, cause } => write!(
                f,
                "Gitto couldn't {action} its list of repositories: {}",
                reason(cause.as_ref())
            ),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::NotAGitRepository { .. } => None,
            Error::List { cause, .. } => Some(cause.as_ref()),
        }
    }
}

impl Error {
    /// A failure to `action` the list, because of `cause`.
    pub fn list(action: ListAction, cause: rusqlite::Error) -> Error {
        Error::List {
            action,
            cause: Arc::new(cause),
        }
    }

    /// Whether this is a bug to report rather than something to tell the user: a list that can't
    /// be read or changed is (TS's `INTERNAL_SERVER_ERROR`), a folder that isn't a repository
    /// isn't (`BAD_REQUEST`).
    pub fn is_bug(&self) -> bool {
        matches!(self, Error::List { .. })
    }
}

/// What was done with the list of repositories when it failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ListAction {
    Read,
    Update,
}

impl ListAction {
    /// As the message says it: `read` or `update`.
    pub fn as_str(self) -> &'static str {
        match self {
            ListAction::Read => "read",
            ListAction::Update => "update",
        }
    }
}

impl fmt::Display for ListAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The innermost error's message, with a full stop.
///
/// In TS that's the error node:sqlite threw, with SQLite's message, which Drizzle's own wraps in
/// its query. Here it's the [`rusqlite::Error`], whose message is SQLite's: its source is only
/// SQLite's result code, said in general terms (`Error code 19: A constraint violation occurred`
/// for `UNIQUE constraint failed: repositories.path`), so the chain is followed no further.
pub fn reason(error: &(dyn std::error::Error + 'static)) -> String {
    let mut inner = error;
    while !inner.is::<rusqlite::Error>() {
        match inner.source() {
            Some(source) => inner = source,
            None => break,
        }
    }
    let message = inner.to_string();
    let message = message.trim();
    if message.ends_with(['.', '!', '?']) {
        message.to_owned()
    } else {
        format!("{message}.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An error that wraps `source`, as Drizzle's query error wraps SQLite's.
    #[derive(Debug)]
    struct Wrapping {
        message: &'static str,
        source: Option<Box<dyn std::error::Error + 'static>>,
    }

    impl fmt::Display for Wrapping {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(self.message)
        }
    }

    impl std::error::Error for Wrapping {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            self.source.as_deref()
        }
    }

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
    fn takes_sqlites_message_through_the_list_error() {
        let error = Error::list(
            ListAction::Update,
            sqlite_error(5, Some("database is locked")),
        );
        assert_eq!(reason(&error), "database is locked.");
    }

    #[test]
    fn takes_the_innermost_message() {
        let error = Wrapping {
            message: "Failed query: select 1",
            source: Some(Box::new(Wrapping {
                message: "disk I/O error",
                source: None,
            })),
        };
        assert_eq!(reason(&error), "disk I/O error.");
    }

    #[test]
    fn stops_at_sqlites_error() {
        let unique = || sqlite_error(19, Some("UNIQUE constraint failed: repositories.path"));
        // Its source, the result code, says it in general terms.
        let code = std::error::Error::source(&unique()).unwrap().to_string();
        assert!(code.starts_with("Error code 19: "), "{code}");
        let error = Wrapping {
            message: "Failed query: insert into \"repositories\"",
            source: Some(Box::new(unique())),
        };
        assert_eq!(
            reason(&error),
            "UNIQUE constraint failed: repositories.path."
        );
        // Without a message from SQLite, the result code's is all there is.
        assert_eq!(
            reason(&sqlite_error(5, None)),
            "Error code 5: database is locked."
        );
    }

    #[test]
    fn trims_the_message_and_keeps_its_own_punctuation() {
        let with = |message| Wrapping {
            message,
            source: None,
        };
        assert_eq!(
            reason(&with("  no such table: repositories \n")),
            "no such table: repositories."
        );
        assert_eq!(reason(&with("Done.")), "Done.");
        assert_eq!(reason(&with("Really!")), "Really!");
        assert_eq!(reason(&with("Why?")), "Why?");
    }
}
