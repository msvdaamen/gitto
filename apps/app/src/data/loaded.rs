//! A piece of data as it's loaded: what's there, why it last failed, and whether it's loading. A
//! [`Repository`](super::repository::Repository) holds one for each thing it loads, as a field,
//! and loads it (see `Repository::load`); views read it in `render`.

use futures::channel::oneshot;
use gpui_kit::Task;

use super::client::QueryError;

/// What's loaded of something (see the module's doc). A load that brings data equal to what's
/// there keeps what's there, so nothing selected from it changes; data too big to compare is an
/// `Opaque`.
pub struct Loaded<T> {
    data: Option<T>,
    error: Option<QueryError>,
    /// The load under way: dropped to cancel it.
    load: Option<Task<()>>,
    /// Told once the load under way is done, or cancelled.
    waiters: Vec<oneshot::Sender<()>>,
    /// Whether it's to be loaded again when it's next shown: the repository changed while it
    /// wasn't, say.
    stale: bool,
    /// How many times it's been loaded or set, equal data or not.
    updates: u64,
}

impl<T> Default for Loaded<T> {
    fn default() -> Self {
        Loaded {
            data: None,
            error: None,
            load: None,
            waiters: Vec::new(),
            stale: false,
            updates: 0,
        }
    }
}

impl<T> Loaded<T> {
    /// `data`, as if it had loaded just now.
    pub fn with(data: T) -> Self {
        Loaded {
            data: Some(data),
            updates: 1,
            ..Loaded::default()
        }
    }

    /// What's loaded; `None` until it is. What was loaded before stays while it loads again, and
    /// if that fails.
    pub fn data(&self) -> Option<&T> {
        self.data.as_ref()
    }

    /// Why the last load failed, if it did.
    pub fn error(&self) -> Option<&QueryError> {
        self.error.as_ref()
    }

    /// Nothing loaded yet, and no error.
    pub fn is_pending(&self) -> bool {
        self.data.is_none() && self.error.is_none()
    }

    pub fn is_error(&self) -> bool {
        self.error.is_some()
    }

    /// Loaded, and the last load didn't fail.
    pub fn is_success(&self) -> bool {
        self.data.is_some() && self.error.is_none()
    }

    /// Whether it's loading, the first time or again.
    pub fn is_fetching(&self) -> bool {
        self.load.is_some()
    }

    /// Loading for the first time: nothing of it to show yet.
    pub fn is_loading(&self) -> bool {
        self.is_fetching() && self.data.is_none()
    }

    /// Loading again, with what was loaded before on show.
    pub fn is_refetching(&self) -> bool {
        self.is_fetching() && self.data.is_some()
    }

    /// Whether it's to be loaded when it's next shown: it never was, or it's out of date.
    pub fn is_stale(&self) -> bool {
        self.stale || (self.data.is_none() && self.error.is_none())
    }

    /// How many times it's been loaded or set: changes with each load that went through, equal
    /// data or not.
    pub fn updates(&self) -> u64 {
        self.updates
    }

    /// Whether anything asked for it: it's loaded, loading, or failed to.
    pub fn is_used(&self) -> bool {
        self.data.is_some() || self.error.is_some() || self.load.is_some()
    }

    /// Marks it out of date, to be loaded again when it's next shown.
    pub fn mark_stale(&mut self) {
        self.stale = true;
    }

    /// Sets its data, as if it had loaded it; whether that changed it.
    pub fn set(&mut self, data: T) -> bool
    where
        T: PartialEq,
    {
        let changed = self.data.as_ref() != Some(&data) || self.error.is_some();
        if self.data.as_ref() != Some(&data) {
            self.data = Some(data);
        }
        self.error = None;
        self.stale = false;
        self.updates += 1;
        changed
    }

    /// Changes its data with `update`, if it has any; whether that changed it.
    pub fn update(&mut self, update: impl FnOnce(&T) -> T) -> bool
    where
        T: PartialEq,
    {
        match self.data.as_ref().map(update) {
            Some(data) => self.set(data),
            None => false,
        }
    }

    /// Starts `load`, the task loading it, in place of (and so cancelling) one under way.
    pub(crate) fn start(&mut self, load: Task<()>) {
        self.load = Some(load);
    }

    /// A receiver told once the load under way is done; `None` if none is.
    pub fn wait(&mut self) -> Option<oneshot::Receiver<()>> {
        self.load.as_ref()?;
        let (done, wait) = oneshot::channel();
        self.waiters.push(done);
        Some(wait)
    }

    /// Takes what a load brought: the data (kept as it was if it's equal), or why it failed (with
    /// what was there before kept).
    pub(crate) fn settle(&mut self, result: Result<T, QueryError>)
    where
        T: PartialEq,
    {
        match result {
            Ok(data) => {
                self.set(data);
            }
            Err(error) => self.error = Some(error),
        }
        self.load = None;
        for waiter in self.waiters.drain(..) {
            let _ = waiter.send(());
        }
    }

    /// Stops the load under way, if any, leaving the data as it was: so one that started before a
    /// change can't overwrite what the change set. Whether one was.
    pub fn cancel(&mut self) -> bool {
        if self.load.take().is_none() {
            return false;
        }
        for waiter in self.waiters.drain(..) {
            let _ = waiter.send(());
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_what_it_had_when_a_load_brings_the_same() {
        let mut loaded = Loaded::with(vec![1, 2]);
        let before = loaded.data().unwrap().as_ptr();
        assert!(!loaded.set(vec![1, 2]));
        assert_eq!(loaded.data().unwrap().as_ptr(), before);
        assert_eq!(loaded.updates(), 2);
        assert!(loaded.set(vec![3]));
    }

    #[test]
    fn keeps_what_it_had_when_a_load_fails() {
        let mut loaded = Loaded::with(1);
        loaded.settle(Err(QueryError::msg("No.")));
        assert_eq!(loaded.data(), Some(&1));
        assert_eq!(loaded.error().unwrap().message(), "No.");
        assert!(!loaded.is_success());
        loaded.settle(Ok(1));
        assert!(loaded.error().is_none());
    }

    #[test]
    fn is_stale_until_its_loaded_and_once_its_marked() {
        let mut loaded = Loaded::<i32>::default();
        assert!(loaded.is_stale() && loaded.is_pending() && !loaded.is_used());
        loaded.set(1);
        assert!(!loaded.is_stale());
        loaded.mark_stale();
        assert!(loaded.is_stale());
        loaded.set(1);
        assert!(!loaded.is_stale());
    }
}
