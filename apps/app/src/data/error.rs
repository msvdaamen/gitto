//! Why loading something, or changing a repository, failed: [`QueryError`].

use std::sync::Arc;

use gitto_git::{ApiCode, ApiError};

/// Why a load, or a change, failed: shared, so every view showing it has the same one.
/// Usually git's [`ApiError`] (see [`QueryError::api`]).
#[derive(Clone)]
pub struct QueryError(Arc<dyn std::error::Error + Send + Sync>);

impl QueryError {
    pub fn new(error: impl std::error::Error + Send + Sync + 'static) -> Self {
        QueryError(Arc::new(error))
    }

    /// An error that's only its message, e.g. a test's.
    pub fn msg(message: impl Into<String>) -> Self {
        QueryError::new(Message(message.into()))
    }

    /// What the user is told.
    pub fn message(&self) -> String {
        self.0.to_string()
    }

    /// The error as git's API made it, with its code; `None` for one that isn't.
    pub fn api(&self) -> Option<&ApiError> {
        self.0.downcast_ref::<ApiError>()
    }

    /// Whether it's the API's error with `code` (`hasCode` in `lib/errors.ts`).
    pub fn has_code(&self, code: ApiCode) -> bool {
        self.api().is_some_and(|error| error.code() == code)
    }

    pub fn inner(&self) -> &(dyn std::error::Error + Send + Sync + 'static) {
        &*self.0
    }
}

/// The same error, not merely one that says the same.
impl PartialEq for QueryError {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl std::fmt::Debug for QueryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&*self.0, f)
    }
}

impl std::fmt::Display for QueryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&*self.0, f)
    }
}

impl From<ApiError> for QueryError {
    fn from(error: ApiError) -> Self {
        QueryError::new(error)
    }
}

/// As the API shows it (see `ApiError`), for git's errors that didn't come through a `GitRepo`
/// method.
impl From<gitto_git::Error> for QueryError {
    fn from(error: gitto_git::Error) -> Self {
        QueryError::new(ApiError::from(error))
    }
}

impl From<gitto_repository::Error> for QueryError {
    fn from(error: gitto_repository::Error) -> Self {
        QueryError::new(error)
    }
}

impl From<std::io::Error> for QueryError {
    fn from(error: std::io::Error) -> Self {
        QueryError::new(error)
    }
}

/// With its causes (`{:#}`), which its `Display` alone leaves out.
impl From<anyhow::Error> for QueryError {
    fn from(error: anyhow::Error) -> Self {
        QueryError::msg(format!("{error:#}"))
    }
}

#[derive(Debug)]
struct Message(String);

impl std::fmt::Display for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Message {}
