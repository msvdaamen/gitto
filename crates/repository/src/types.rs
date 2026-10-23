//! The repository type. Ported from `packages/repository/src/types.ts`.

use serde::{Deserialize, Serialize};

/// A git repository added to Gitto: a tab.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Repository {
    /// A UUID v7, made when it was added.
    pub id: String,
    /// Its folder's name.
    pub name: String,
    /// Its folder: absolute, as it was added, without following symlinks.
    pub path: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_serialized_as_in_ts() {
        let repository = Repository {
            id: "01999a3c-0000-7000-8000-000000000001".to_owned(),
            name: "gitto".to_owned(),
            path: "/home/me/gitto".to_owned(),
        };
        assert_eq!(
            serde_json::to_string(&repository).unwrap(),
            r#"{"id":"01999a3c-0000-7000-8000-000000000001","name":"gitto","path":"/home/me/gitto"}"#
        );
    }
}
