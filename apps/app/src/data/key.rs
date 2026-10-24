//! A query's key: what it's cached under, and what invalidating a prefix of it reaches. TanStack
//! Query's array keys, as segments.

use std::fmt;
use std::sync::Arc;

use gpui_kit::SharedString;

/// A query's key, e.g. `["git", <repository id>, "uncommitted", "status"]`. Invalidating a key
/// reaches every query whose key starts with it (see [`QueryKey::starts_with`]).
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct QueryKey(Arc<[SharedString]>);

impl QueryKey {
    pub fn new<I, S>(segments: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<SharedString>,
    {
        QueryKey(segments.into_iter().map(Into::into).collect())
    }

    /// This key, with `segment` after it.
    pub fn with(&self, segment: impl Into<SharedString>) -> Self {
        QueryKey(
            self.0
                .iter()
                .cloned()
                .chain(std::iter::once(segment.into()))
                .collect(),
        )
    }

    pub fn segments(&self) -> &[SharedString] {
        &self.0
    }

    /// Whether `prefix` is this key's first segments, or the whole of it.
    pub fn starts_with(&self, prefix: &QueryKey) -> bool {
        self.0.starts_with(&prefix.0)
    }
}

impl fmt::Debug for QueryKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.0.iter()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reaches_the_keys_it_starts() {
        let repository = QueryKey::new(["git", "repo"]);
        let status = repository.with("uncommitted").with("status");

        assert_eq!(
            status,
            QueryKey::new(["git", "repo", "uncommitted", "status"])
        );
        assert!(status.starts_with(&repository));
        assert!(repository.starts_with(&repository));
        assert!(!repository.starts_with(&status));
        assert!(!QueryKey::new(["git", "other"]).starts_with(&repository));
    }
}
