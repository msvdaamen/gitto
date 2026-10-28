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

/// `T`, shared, and equal only to clones of itself (see the module's doc). Either a value of its
/// own, or a part of another `Opaque`'s, which it shares rather than copies (see
/// [`part`](Self::part)).
pub struct Opaque<T: 'static>(Arc<dyn Holds<T>>);

/// What an `Opaque` holds its value in.
trait Holds<T>: Send + Sync {
    fn value(&self) -> &T;
}

/// A value of its own.
struct Whole<T>(T);

impl<T: Send + Sync> Holds<T> for Whole<T> {
    fn value(&self) -> &T {
        &self.0
    }
}

/// A part of another `Opaque`'s value, which it keeps.
struct Part<W: 'static, T> {
    whole: Opaque<W>,
    part: fn(&W) -> &T,
}

impl<W: 'static, T> Holds<T> for Part<W, T> {
    fn value(&self) -> &T {
        (self.part)(self.whole.value())
    }
}

impl<T: Send + Sync + 'static> Opaque<T> {
    pub fn new(value: T) -> Self {
        Opaque(Arc::new(Whole(value)))
    }
}

impl<T: 'static> Opaque<T> {
    /// The part `part` picks of `whole`'s value, shared rather than copied: e.g. one of a status's
    /// lists of files. It's another `Opaque`, equal only to its own clones, not to `whole`, nor to
    /// another one of the same part.
    pub fn part<W: 'static>(whole: &Opaque<W>, part: fn(&W) -> &T) -> Self {
        Opaque(Arc::new(Part {
            whole: whole.clone(),
            part,
        }))
    }

    pub fn value(&self) -> &T {
        self.0.value()
    }

    /// Whether `a` and `b` are the same value, not merely equal ones.
    pub fn ptr_eq(a: &Self, b: &Self) -> bool {
        Arc::ptr_eq(&a.0, &b.0)
    }
}

impl<T: 'static> Clone for Opaque<T> {
    fn clone(&self) -> Self {
        Opaque(Arc::clone(&self.0))
    }
}

impl<T: 'static> PartialEq for Opaque<T> {
    fn eq(&self, other: &Self) -> bool {
        Opaque::ptr_eq(self, other)
    }
}

impl<T: 'static> Deref for Opaque<T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.value()
    }
}

impl<T: fmt::Debug + 'static> fmt::Debug for Opaque<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Opaque").field(self.value()).finish()
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

    #[test]
    fn shares_a_part_of_another() {
        let pair = Opaque::new((vec!["a.txt".to_owned()], 1));
        let files = Opaque::part(&pair, |pair| &pair.0);
        assert_eq!(*files, ["a.txt"]);
        assert!(std::ptr::eq(files.value(), &pair.value().0));
        assert_eq!(files, files.clone());
        assert_ne!(files, Opaque::part(&pair, |pair| &pair.0));
    }
}
