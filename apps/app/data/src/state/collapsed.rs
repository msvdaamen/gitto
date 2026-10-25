//! Which sidebar sections and folders are collapsed. Ported from `hooks/collapsed.ts`.

use std::collections::HashMap;
use std::rc::Rc;

use serde_json::{Map, Value};

use crate::services::LocalStorage;

/// Whether each section or folder, by its id, is collapsed.
type CollapsedState = HashMap<String, bool>;

/// Where a repository's state is saved, by its id.
fn storage_key(repository_id: &str) -> String {
    format!("gitto-sidebar-collapsed:{repository_id}")
}

/// The state saved as `saved` (JSON, as `localStorage` had it); none if it's unreadable.
fn parse_state(saved: Option<&str>) -> CollapsedState {
    let Some(Ok(Value::Object(saved))) = saved.map(serde_json::from_str::<Value>) else {
        return CollapsedState::new();
    };
    saved
        .into_iter()
        .filter_map(|(id, value)| truthiness(&value).map(|collapsed| (id, collapsed)))
        .collect()
}

/// Whether JavaScript takes `value` as true; `None` for `null`, which `??` passes over.
fn truthiness(value: &Value) -> Option<bool> {
    match value {
        Value::Null => None,
        Value::Bool(value) => Some(*value),
        Value::Number(number) => Some(number.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan())),
        Value::String(text) => Some(!text.is_empty()),
        Value::Array(_) | Value::Object(_) => Some(true),
    }
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
            state: parse_state(storage.get_item(&storage_key(&repository_id)).as_deref()),
            defaults: defaults
                .into_iter()
                .map(|(id, collapsed)| (id.to_owned(), collapsed))
                .collect(),
            repository_id,
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
        let _ = self.storage.set_item(
            &storage_key(&self.repository_id),
            Value::Object(saved).to_string(),
        );
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
}
