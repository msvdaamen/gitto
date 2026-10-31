//! The branches, remote branches and tags, as the UI gets them. Ported from `refs/schema.ts`.

use serde::{Deserialize, Serialize};

/// Which kind of ref: a local branch, a remote branch or a tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RefKind {
    Local,
    Remote,
    Tag,
}

impl RefKind {
    /// As TS spells it, e.g. `local`.
    pub fn as_str(self) -> &'static str {
        match self {
            RefKind::Local => "local",
            RefKind::Remote => "remote",
            RefKind::Tag => "tag",
        }
    }
}

/// A branch, remote branch or tag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ref {
    /// Short name, e.g. `main`, `origin/main`, `v1.0.0`.
    pub name: String,
    /// Full ref name, e.g. `refs/heads/main`.
    pub full_name: String,
    pub kind: RefKind,
    pub sha: String,
    pub current: bool,
    pub upstream: Option<String>,
    pub ahead: u64,
    pub behind: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spells_each_kind_as_its_json_does() {
        use RefKind::*;
        // Doesn't compile once there's another kind, until it's listed here too.
        let _listed = |kind| match kind {
            Local | Remote | Tag => (),
        };
        for kind in [Local, Remote, Tag] {
            assert_eq!(serde_json::to_value(kind).unwrap(), kind.as_str());
        }
    }
}
