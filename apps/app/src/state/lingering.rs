//! A value that lingers once it's gone. Ported from `hooks/lingering.ts`.

/// A value, or the last one it had while it's `None`: what a dialog is about, still named while
/// it closes, rather than going blank as it fades. Its owner [`update`](Lingering::update)s it
/// with the value as it is now, and shows what that returns.
#[derive(Debug, Clone)]
pub struct Lingering<T> {
    last: Option<T>,
}

impl<T> Default for Lingering<T> {
    fn default() -> Self {
        Lingering { last: None }
    }
}

impl<T> Lingering<T> {
    /// `value`, or the last one it had, if it's `None`.
    pub fn update(&mut self, value: Option<T>) -> Option<&T> {
        if value.is_some() {
            self.last = value;
        }
        self.last.as_ref()
    }

    /// What it was last updated to, or the last value it had.
    pub fn get(&self) -> Option<&T> {
        self.last.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_last_value_while_its_gone() {
        let mut shown = Lingering::default();
        assert_eq!(shown.update(Some("main")), Some(&"main"));
        assert_eq!(shown.update(None), Some(&"main"));
        assert_eq!(Lingering::<&str>::default().update(None), None);
    }
}
