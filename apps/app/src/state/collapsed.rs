//! Which sidebar sections and folders are collapsed: gitto-ui's `hooks::collapsed` (ported from
//! `hooks/collapsed.ts`), saved in the app's [`LocalStorage`].

use std::rc::Rc;

use gitto_ui::hooks::collapsed;

use crate::services::LocalStorage;

/// Which sidebar sections and folders are collapsed in a repository, saved (as `localStorage`
/// kept it) so it survives restarts. Ids without a saved state fall back to `defaults`, then to
/// expanded. Made again for another repository, which loads that one's state.
#[derive(Debug, Clone)]
pub struct Collapsed {
    state: collapsed::Collapsed,
    storage: Rc<LocalStorage>,
}

impl Collapsed {
    pub fn new(
        repository_id: impl Into<String>,
        defaults: impl IntoIterator<Item = (&'static str, bool)>,
        storage: Rc<LocalStorage>,
    ) -> Self {
        let defaults = defaults
            .into_iter()
            .map(|(id, collapsed)| (id.to_owned(), collapsed))
            .collect();
        Collapsed {
            state: collapsed::Collapsed::load(&repository_id.into(), defaults, &&*storage),
            storage,
        }
    }

    pub fn repository_id(&self) -> &str {
        self.state.repository_id()
    }

    pub fn is_collapsed(&self, id: &str) -> bool {
        self.state.is_collapsed(id)
    }

    pub fn toggle(&mut self, id: &str) {
        self.state.toggle(id, &mut &*self.storage);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mount(storage: &Rc<LocalStorage>, repository_id: &str) -> Collapsed {
        Collapsed::new(repository_id, [("tags", true)], Rc::clone(storage))
    }

    #[test]
    fn falls_back_to_the_defaults_then_to_expanded() {
        let storage = Rc::new(LocalStorage::in_memory());
        let collapsed = mount(&storage, "repo");
        assert!(collapsed.is_collapsed("tags"));
        assert!(!collapsed.is_collapsed("refs/heads/feature"));
    }

    #[test]
    fn keeps_toggled_state_across_remounts() {
        let storage = Rc::new(LocalStorage::in_memory());
        let mut first = mount(&storage, "repo");
        first.toggle("tags");
        first.toggle("refs/heads/feature");

        let second = mount(&storage, "repo");
        assert!(!second.is_collapsed("tags"));
        assert!(second.is_collapsed("refs/heads/feature"));
    }

    #[test]
    fn keeps_a_separate_state_per_repository() {
        let storage = Rc::new(LocalStorage::in_memory());
        let mut a = Collapsed::new("a", [], Rc::clone(&storage));
        a.toggle("remotes");
        assert!(!Collapsed::new("b", [], Rc::clone(&storage)).is_collapsed("remotes"));
        assert!(Collapsed::new("a", [], Rc::clone(&storage)).is_collapsed("remotes"));
    }

    #[test]
    fn ignores_unreadable_saved_state() {
        let storage = Rc::new(LocalStorage::in_memory());
        storage
            .set_item("gitto-sidebar-collapsed:repo", "not json")
            .unwrap();
        assert!(!Collapsed::new("repo", [], storage).is_collapsed("remotes"));
    }
}
