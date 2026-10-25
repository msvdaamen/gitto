//! Why loading something, or changing a repository, failed: [`DataError`].

use std::sync::Arc;

use gitto_git::{ApiCode, ApiError};

/// Why a load, or a change, failed: shared, so every view showing it has the same one.
/// Usually git's [`ApiError`] (see [`DataError::api`]).
#[derive(Clone)]
pub struct DataError(Arc<dyn std::error::Error + Send + Sync>);

impl DataError {
    pub fn new(error: impl std::error::Error + Send + Sync + 'static) -> Self {
        DataError(Arc::new(error))
    }

    /// An error that's only its message, e.g. a test's.
    pub fn msg(message: impl Into<String>) -> Self {
        DataError::new(Message(message.into()))
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
}

/// The same error, not merely one that says the same.
impl PartialEq for DataError {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl std::fmt::Debug for DataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&*self.0, f)
    }
}

impl std::fmt::Display for DataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&*self.0, f)
    }
}

impl From<ApiError> for DataError {
    fn from(error: ApiError) -> Self {
        DataError::new(error)
    }
}

/// As the API shows it (see `ApiError`), for git's errors that didn't come through a `GitRepo`
/// method.
impl From<gitto_git::Error> for DataError {
    fn from(error: gitto_git::Error) -> Self {
        DataError::new(ApiError::from(error))
    }
}

impl From<gitto_repository::Error> for DataError {
    fn from(error: gitto_repository::Error) -> Self {
        DataError::new(error)
    }
}

impl From<std::io::Error> for DataError {
    fn from(error: std::io::Error) -> Self {
        DataError::new(error)
    }
}

/// With its causes (`{:#}`), which its `Display` alone leaves out.
impl From<anyhow::Error> for DataError {
    fn from(error: anyhow::Error) -> Self {
        DataError::msg(format!("{error:#}"))
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
