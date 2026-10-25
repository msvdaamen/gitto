//! What's loaded that's shared rather than compared. Ported from `git/queries/opaque.ts`.
//!
//! The data layer compares what it loads with what it had, and a view what it shows with what it
//! showed, so data that didn't change doesn't render again (see `super::loaded`). For a list of thousands of files, comparing takes long enough to stall the
//! UI, and gains nothing: the list is replaced as a whole when it changes, never changed in place.
//! An `Opaque` is equal only to itself, a clone of the same `Arc`: comparing it costs nothing, and
//! a status that git says is unchanged keeps the same one (see `super::repository`).

use std::fmt;
use std::ops::Deref;
use std::sync::Arc;

/// `T`, shared, and equal only to clones of itself (see the module's doc).
pub struct Opaque<T>(Arc<T>);

impl<T> Opaque<T> {
    pub fn new(value: T) -> Self {
        Opaque(Arc::new(value))
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

/// `select`, run once per `Opaque` it's given: what it picks of the last one is kept, and handed
/// back while it's given the same one again. For what a view picks of what's loaded that takes
/// work (filtering a list of files, say): a view picks again whenever it renders, and a pick
/// made again would be another `Opaque`, unequal to the last.
pub fn memoized<T: 'static, S: Clone + 'static>(
    select: impl Fn(&Opaque<T>) -> S + 'static,
) -> impl Fn(&Opaque<T>) -> S + 'static {
    let last: std::cell::RefCell<Option<(Opaque<T>, S)>> = std::cell::RefCell::new(None);
    move |value| {
        if let Some((of, selected)) = &*last.borrow()
            && Opaque::ptr_eq(of, value)
        {
            return selected.clone();
        }
        let selected = select(value);
        *last.borrow_mut() = Some((value.clone(), selected.clone()));
        selected
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

    #[test]
    fn selects_once_per_value() {
        let select = memoized(|files: &Opaque<Vec<String>>| Opaque::new(files.len()));
        let files = Opaque::new(vec!["a.txt".to_owned()]);
        assert_eq!(select(&files), select(&files.clone()));
        assert_ne!(select(&files), select(&Opaque::new(vec![])));
    }
}
