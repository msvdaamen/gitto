//! Telling when a value changes, to reset what's of the one before. Ported from
//! `hooks/reset-on.ts`.

/// A value it follows (see [`ResetOn::changed`]): for state that's of one repository, like a
/// dialog's fields or which item's menu is open, which another repository shouldn't get. Most
/// views reset such state where they're told the repository changed (`set_repository`); this is
/// for what's only seen in `render`, or in an observer.
#[derive(Debug, Clone)]
pub struct ResetOn<T> {
    value: T,
}

impl<T: PartialEq> ResetOn<T> {
    /// Following `value`, as it is at first.
    pub fn new(value: T) -> Self {
        ResetOn { value }
    }

    /// Whether `value` is another than the one before, which it then follows: whether to reset.
    /// Not at first, as `new` took the first.
    pub fn changed(&mut self, value: T) -> bool {
        if self.value == value {
            return false;
        }
        self.value = value;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn says_when_it_changes_though_not_at_first() {
        let mut repository = ResetOn::new("a");
        assert!(!repository.changed("a"));
        assert!(repository.changed("b"));
        assert!(!repository.changed("b"));
    }
}
