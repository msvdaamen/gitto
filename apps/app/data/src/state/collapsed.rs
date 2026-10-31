//! Which sidebar sections and folders are collapsed. Ported from `hooks/collapsed.ts`.

use std::collections::HashMap;
use std::rc::Rc;

use gitto_git::input::RepositoryId;
use serde_json::{Map, Value};

use crate::services::LocalStorage;

/// Whether each section or folder, by its id, is collapsed.
type CollapsedState = HashMap<String, bool>;

/// Where a repository's state is saved, by its id.
fn storage_key(repository_id: &RepositoryId) -> String {
    format!("gitto-sidebar-collapsed:{repository_id}")
}

/// The state saved as `saved`, a JSON object of booleans; none if it's unreadable, and an id
/// saved as anything but a boolean has none.
fn parse_state(saved: Option<&str>) -> CollapsedState {
    let Some(Ok(Value::Object(saved))) = saved.map(serde_json::from_str::<Value>) else {
        return CollapsedState::new();
    };
    saved
        .into_iter()
        .filter_map(|(id, value)| value.as_bool().map(|collapsed| (id, collapsed)))
        .collect()
}

/// Which sidebar sections and folders are collapsed in a repository, saved (as `localStorage`
/// kept it) so it survives restarts. Ids without a saved state fall back to `defaults`, then to
/// expanded. Made again for another repository, which loads that one's state.
#[derive(Debug, Clone)]
pub struct Collapsed {
    repository_id: RepositoryId,
    state: CollapsedState,
    defaults: CollapsedState,
    storage: Rc<LocalStorage>,
}

impl Collapsed {
    pub fn new(
        repository_id: &RepositoryId,
        defaults: impl IntoIterator<Item = (&'static str, bool)>,
        storage: Rc<LocalStorage>,
    ) -> Self {
        let repository_id = repository_id.clone();
        Collapsed {
            state: parse_state(storage.get_item(&storage_key(&repository_id)).as_deref()),
            defaults: defaults
                .into_iter()
                .map(|(id, collapsed)| (id.to_owned(), collapsed))
                .collect(),
            repository_id,
            storage,
        }
    }

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

    /// Collapses `id` if it's expanded, and expands it if it's collapsed; saved.
    pub fn toggle(&mut self, id: &str) {
        let collapsed = !self.is_collapsed(id);
        self.state.insert(id.to_owned(), collapsed);
        let saved: Map<String, Value> = self
            .state
            .iter()
            .map(|(id, collapsed)| (id.clone(), Value::Bool(*collapsed)))
            .collect();
        // Storage full or unavailable: the state still applies until the app closes.
        self.storage.set_item(
            &storage_key(&self.repository_id),
            Value::Object(saved).to_string(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The repository with the id `n` (1 or 2, say).
    fn repo(n: u8) -> RepositoryId {
        RepositoryId::parse(format!("01890a5d-ac96-774b-bcce-b302099a805{n}")).unwrap()
    }

    fn mount(storage: &Rc<LocalStorage>, repository_id: &RepositoryId) -> Collapsed {
        Collapsed::new(repository_id, [("tags", true)], Rc::clone(storage))
    }

    #[test]
    fn falls_back_to_the_defaults_then_to_expanded() {
        let storage = Rc::new(LocalStorage::in_memory());
        let collapsed = mount(&storage, &repo(1));
        assert!(collapsed.is_collapsed("tags"));
        assert!(!collapsed.is_collapsed("refs/heads/feature"));
    }

    #[test]
    fn keeps_toggled_state_across_remounts() {
        let storage = Rc::new(LocalStorage::in_memory());
        let mut first = mount(&storage, &repo(1));
        first.toggle("tags");
        first.toggle("refs/heads/feature");

        let second = mount(&storage, &repo(1));
        assert!(!second.is_collapsed("tags"));
        assert!(second.is_collapsed("refs/heads/feature"));
    }

    #[test]
    fn keeps_a_separate_state_per_repository() {
        let storage = Rc::new(LocalStorage::in_memory());
        let mut a = Collapsed::new(&repo(1), [], Rc::clone(&storage));
        a.toggle("remotes");
        assert!(!Collapsed::new(&repo(2), [], Rc::clone(&storage)).is_collapsed("remotes"));
        assert!(Collapsed::new(&repo(1), [], Rc::clone(&storage)).is_collapsed("remotes"));
    }

    #[test]
    fn ignores_unreadable_saved_state() {
        let storage = Rc::new(LocalStorage::in_memory());
        storage.set_item(&storage_key(&repo(1)), "not json");
        assert!(!Collapsed::new(&repo(1), [], storage).is_collapsed("remotes"));
    }

    #[test]
    fn saves_it_under_the_key_the_electron_app_did() {
        assert_eq!(
            storage_key(&repo(1)),
            "gitto-sidebar-collapsed:01890a5d-ac96-774b-bcce-b302099a8051"
        );
    }

    #[test]
    fn reads_the_state_the_electron_app_saved() {
        let state = parse_state(Some(r#"{"tags":false,"remotes":true,"gone":null}"#));
        let expected: CollapsedState = [("tags".to_owned(), false), ("remotes".to_owned(), true)]
            .into_iter()
            .collect();
        assert_eq!(state, expected);
        assert!(parse_state(Some("[true]")).is_empty());
        assert!(parse_state(None).is_empty());
    }

    #[test]
    fn has_no_state_for_an_id_saved_as_anything_but_a_boolean() {
        let state = parse_state(Some(r#"{"tags":true,"one":1,"yes":"yes","list":[true]}"#));
        let expected: CollapsedState = [("tags".to_owned(), true)].into_iter().collect();
        assert_eq!(state, expected);
    }
}
