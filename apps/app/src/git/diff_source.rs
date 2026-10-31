//! From `git/diff-source.ts` (see the module's doc).

/// Where the changes in a file on show are from: a commit or a stash, which never change, or one
/// side of the uncommitted changes, which change on disk: what isn't staged (the working tree
/// compared to the index), or what is (the index compared to HEAD).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DiffSource {
    Commit { sha: String },
    Stash { sha: String },
    Unstaged,
    Staged,
}

impl DiffSource {
    pub fn is_uncommitted(&self) -> bool {
        matches!(self, DiffSource::Unstaged | DiffSource::Staged)
    }

    fn kind(&self) -> &'static str {
        match self {
            DiffSource::Commit { .. } => "commit",
            DiffSource::Stash { .. } => "stash",
            DiffSource::Unstaged => "unstaged",
            DiffSource::Staged => "staged",
        }
    }

    /// Names a file in this source, to tell whether it's the one on show: the same path can be
    /// both in the staged and the unstaged changes (`diffFileKey`).
    pub fn file_key(&self, path: &str) -> String {
        let sha = match self {
            DiffSource::Commit { sha } | DiffSource::Stash { sha } => sha.as_str(),
            DiffSource::Unstaged | DiffSource::Staged => "",
        };
        format!("{}:{sha}:{path}", self.kind())
    }

    /// Whether `self` and `other` are the same commit, stash, or side of the uncommitted changes
    /// (`isSameSource`).
    pub fn is_same(&self, other: &DiffSource) -> bool {
        self.file_key("") == other.file_key("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tells_the_sides_apart() {
        assert_eq!(DiffSource::Unstaged.file_key("a.txt"), "unstaged::a.txt");
        assert_ne!(
            DiffSource::Unstaged.file_key("a.txt"),
            DiffSource::Staged.file_key("a.txt")
        );
        assert!(
            DiffSource::Commit { sha: "a".into() }.is_same(&DiffSource::Commit { sha: "a".into() })
        );
        assert!(
            !DiffSource::Commit { sha: "a".into() }.is_same(&DiffSource::Stash { sha: "a".into() })
        );
    }
}
