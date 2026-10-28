//! Ported from `hooks/collapsed.ts`: which sidebar sections and folders are collapsed.

use std::collections::HashMap;

use gitto_git::input::RepositoryId;
use serde_json::{Map, Value};

use super::Storage;

/// Whether each section or folder, by its id, is collapsed.
pub type CollapsedState = HashMap<String, bool>;

/// Where a repository's state is saved, by its id.
pub fn storage_key(repository_id: &RepositoryId) -> String {
    format!("gitto-sidebar-collapsed:{repository_id}")
}

/// The state saved as `saved`, a JSON object of booleans; none if it's unreadable, and an id
/// saved as anything but a boolean has none.
pub fn parse_state(saved: Option<&str>) -> CollapsedState {
    let Some(Ok(Value::Object(saved))) = saved.map(serde_json::from_str::<Value>) else {
        return CollapsedState::new();
    };
    saved
        .into_iter()
        .filter_map(|(id, value)| value.as_bool().map(|collapsed| (id, collapsed)))
        .collect()
}

/// Which sidebar sections and folders are collapsed in a repository, saved in a [`Storage`] so it
/// survives restarts. Ids without a saved state fall back to `defaults`, then to expanded.
///
/// One per repository: switching repositories loads that repository's state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Collapsed {
    repository_id: RepositoryId,
    state: CollapsedState,
    defaults: CollapsedState,
}

impl Collapsed {
    /// The state saved for the repository `repository_id`, falling back to `defaults`.
    pub fn load(
        repository_id: &RepositoryId,
        defaults: CollapsedState,
        storage: &dyn Storage,
    ) -> Self {
        Self {
            repository_id: repository_id.clone(),
            state: parse_state(storage.get(&storage_key(repository_id)).as_deref()),
            defaults,
        }
    }

    /// The repository it's the state of.
    pub fn repository_id(&self) -> &RepositoryId {
        &self.repository_id
    }

    pub fn is_collapsed(&self, id: &str) -> bool {
        self.state
            .get(id)
            .or_else(|| self.defaults.get(id))
            .copied()
            .unwrap_or(false)
    }

    /// Collapses `id` if it's expanded, and expands it if it's collapsed; saved in `storage`.
    pub fn toggle(&mut self, id: &str, storage: &mut dyn Storage) {
        let collapsed = !self.is_collapsed(id);
        self.state.insert(id.to_owned(), collapsed);
        let saved: Map<String, Value> = self
            .state
            .iter()
            .map(|(id, collapsed)| (id.clone(), Value::Bool(*collapsed)))
            .collect();
        // Storage full or unavailable: the state still applies until the app closes.
        let _ = storage.set(
            &storage_key(&self.repository_id),
            &Value::Object(saved).to_string(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::MemoryStorage;

    const REPO: &str = "01900000-0000-7000-8000-000000000001";

    fn id(value: &str) -> RepositoryId {
        RepositoryId::parse(value).unwrap()
    }

    fn defaults(entries: &[(&str, bool)]) -> CollapsedState {
        entries
            .iter()
            .map(|(id, collapsed)| ((*id).to_owned(), *collapsed))
            .collect()
    }

    mod use_collapsed {
        use super::*;

        #[test]
        fn falls_back_to_the_defaults_then_to_expanded() {
            let storage = MemoryStorage::default();
            let collapsed = Collapsed::load(&id(REPO), defaults(&[("tags", true)]), &storage);

            assert!(collapsed.is_collapsed("tags"));
            assert!(!collapsed.is_collapsed("refs/heads/feature"));
        }

        #[test]
        fn keeps_toggled_state_across_remounts() {
            let mut storage = MemoryStorage::default();
            let mut first = Collapsed::load(&id(REPO), defaults(&[("tags", true)]), &storage);
            first.toggle("tags", &mut storage);
            first.toggle("refs/heads/feature", &mut storage);

            let second = Collapsed::load(&id(REPO), defaults(&[("tags", true)]), &storage);
            assert!(!second.is_collapsed("tags"));
            assert!(second.is_collapsed("refs/heads/feature"));
        }

        #[test]
        fn keeps_a_separate_state_per_repository() {
            let mut storage = MemoryStorage::default();
            let mut collapsed = Collapsed::load(&id(REPO), CollapsedState::new(), &storage);

            collapsed.toggle("remotes", &mut storage);
            let collapsed =
                Collapsed::load(&RepositoryId::generate(), CollapsedState::new(), &storage);
            assert!(!collapsed.is_collapsed("remotes"));
            let collapsed = Collapsed::load(&id(REPO), CollapsedState::new(), &storage);
            assert!(collapsed.is_collapsed("remotes"));
        }

        #[test]
        fn ignores_unreadable_saved_state() {
            let mut storage = MemoryStorage::default();
            storage.set(&storage_key(&id(REPO)), "not json").unwrap();
            let collapsed = Collapsed::load(&id(REPO), CollapsedState::new(), &storage);

            assert!(!collapsed.is_collapsed("remotes"));
        }
    }

    #[test]
    fn saves_it_under_the_key_the_electron_app_did() {
        assert_eq!(
            storage_key(&id(REPO)),
            "gitto-sidebar-collapsed:01900000-0000-7000-8000-000000000001"
        );
    }

    #[test]
    fn reads_the_state_the_electron_app_saved() {
        let state = parse_state(Some(r#"{"tags":false,"remotes":true,"gone":null}"#));
        assert_eq!(state, defaults(&[("tags", false), ("remotes", true)]));
        assert!(parse_state(Some("[true]")).is_empty());
        assert!(parse_state(None).is_empty());
    }

    #[test]
    fn has_no_state_for_an_id_saved_as_anything_but_a_boolean() {
        let state = parse_state(Some(r#"{"tags":true,"one":1,"yes":"yes","list":[true]}"#));
        assert_eq!(state, defaults(&[("tags", true)]));
    }
}
