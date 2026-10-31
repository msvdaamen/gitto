//! Which sidebar sections and folders are collapsed. Ported from `hooks/collapsed.ts`.

use std::collections::BTreeMap;
use std::rc::Rc;

use crate::services::LocalStorage;

type CollapsedState = BTreeMap<String, bool>;

fn storage_key(repository_id: &str) -> String {
    format!("gitto-sidebar-collapsed:{repository_id}")
}

fn load(storage: &LocalStorage, repository_id: &str) -> CollapsedState {
    storage
        .get_item(&storage_key(repository_id))
        .and_then(|saved| serde_json::from_str(&saved).ok())
        .unwrap_or_default()
}

/// Which sidebar sections and folders are collapsed in a repository, saved (as `localStorage`
/// kept it) so it survives restarts. Ids without a saved state fall back to `defaults`, then to
/// expanded. Made again for another repository, which loads that one's state.
#[derive(Debug, Clone)]
pub struct Collapsed {
    repository_id: String,
    state: CollapsedState,
    defaults: CollapsedState,
    storage: Rc<LocalStorage>,
}

impl Collapsed {
    pub fn new(
        repository_id: impl Into<String>,
        defaults: impl IntoIterator<Item = (&'static str, bool)>,
        storage: Rc<LocalStorage>,
    ) -> Self {
        let repository_id = repository_id.into();
        Collapsed {
            state: load(&storage, &repository_id),
            repository_id,
            defaults: defaults
                .into_iter()
                .map(|(id, collapsed)| (id.to_owned(), collapsed))
                .collect(),
            storage,
        }
    }

    pub fn repository_id(&self) -> &str {
        &self.repository_id
    }

    pub fn is_collapsed(&self, id: &str) -> bool {
        self.state
            .get(id)
            .or_else(|| self.defaults.get(id))
            .copied()
            .unwrap_or(false)
    }

    /// Collapses `id`, or expands it; it applies even if it can't be saved, until the app closes.
    pub fn toggle(&mut self, id: &str) {
        let collapsed = !self.is_collapsed(id);
        self.state.insert(id.to_owned(), collapsed);
        if let Ok(saved) = serde_json::to_string(&self.state) {
            self.storage
                .set_item(&storage_key(&self.repository_id), saved);
        }
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
        storage.set_item("gitto-sidebar-collapsed:repo", "not json");
        assert!(!Collapsed::new("repo", [], storage).is_collapsed("remotes"));
    }
}
