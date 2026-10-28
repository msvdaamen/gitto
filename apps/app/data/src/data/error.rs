//! Why loading something, or changing a repository, failed: [`DataError`]. What the user is told
//! is its `Display`.

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

    /// The error as git's API made it, with its code; `None` for one that isn't.
    pub fn api(&self) -> Option<&ApiError> {
        self.downcast_ref::<ApiError>()
    }

    /// The error it is, if it's an `E`: also one an `anyhow::Error` held.
    pub fn downcast_ref<E: std::error::Error + Send + Sync + 'static>(&self) -> Option<&E> {
        self.0.downcast_ref::<E>().or_else(|| {
            self.0
                .downcast_ref::<WithCauses>()
                .and_then(|error| error.0.downcast_ref::<E>())
        })
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

/// What the user is told.
impl std::fmt::Display for DataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&*self.0, f)
    }
}

/// The error it wraps, as it is: its causes are that one's.
impl std::error::Error for DataError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.0.source()
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

/// Kept as it is (see [`DataError::downcast_ref`]), and told with its causes.
impl From<anyhow::Error> for DataError {
    fn from(error: anyhow::Error) -> Self {
        DataError::new(WithCauses(error))
    }
}

/// An `anyhow::Error`, told with its causes (`{:#}`), which its `Display` alone leaves out.
struct WithCauses(anyhow::Error);

impl std::fmt::Display for WithCauses {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:#}", self.0)
    }
}

impl std::fmt::Debug for WithCauses {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.0, f)
    }
}

impl std::error::Error for WithCauses {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.0.source()
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

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use super::*;

    #[test]
    fn keeps_an_anyhow_error_and_tells_its_causes() {
        let api = ApiError::from(gitto_git::Error::PushRejected {
            message: "Rejected.".to_owned(),
            forcible: false,
        });
        let message = api.to_string();
        let error = DataError::from(anyhow::Error::new(api).context("Couldn't push"));
        assert!(
            error
                .to_string()
                .starts_with(&format!("Couldn't push: {message}"))
        );
        assert!(error.api().is_some());
        assert!(error.source().is_some());
    }

    #[test]
    fn goes_through_question_marks_into_anyhow() {
        fn fails() -> anyhow::Result<()> {
            Err(DataError::msg("No."))?
        }
        assert_eq!(fails().unwrap_err().to_string(), "No.");
    }
}
