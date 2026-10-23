//! A query's result that's shared rather than compared. Ported from `git/queries/opaque.ts`.
//!
//! The data layer compares what a query loads with what it had, and what a view selects from it
//! with what it selected, so data that didn't change doesn't render again (see
//! `super::client`). For a list of thousands of files, comparing takes long enough to stall the
//! UI, and gains nothing: the list is replaced as a whole when it changes, never changed in place.
//! An `Opaque` is equal only to itself, a clone of the same `Arc`: comparing it costs nothing, and
//! a query that answers "unchanged" hands back the same one (see `super::status`).

use std::fmt;
use std::ops::Deref;
use std::sync::Arc;

/// `T`, shared, and equal only to clones of itself (see the module's doc).
pub struct Opaque<T>(Arc<T>);

impl<T> Opaque<T> {
    pub fn new(value: T) -> Self {
        Opaque(Arc::new(value))
    }

    pub fn value(&self) -> &T {
        &self.0
    }

    /// Whether `a` and `b` are the same value, not merely equal ones.
    pub fn ptr_eq(a: &Self, b: &Self) -> bool {
        Arc::ptr_eq(&a.0, &b.0)
    }
}

impl<T> Clone for Opaque<T> {
    fn clone(&self) -> Self {
        Opaque(Arc::clone(&self.0))
    }
}

impl<T> PartialEq for Opaque<T> {
    fn eq(&self, other: &Self) -> bool {
        Opaque::ptr_eq(self, other)
    }
}

impl<T> Deref for Opaque<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T: fmt::Debug> fmt::Debug for Opaque<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Opaque").field(&self.0).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_only_equal_to_itself() {
        let files = Opaque::new(vec!["a.txt".to_owned()]);
        assert_eq!(files, files.clone());
        assert_ne!(files, Opaque::new(vec!["a.txt".to_owned()]));
        assert_eq!(files[0], "a.txt");
    }
}
