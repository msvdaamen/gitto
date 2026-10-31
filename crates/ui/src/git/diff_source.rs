//! Ported from `git/diff-source.ts`.

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
    /// The commit's or stash's SHA; `None` for the uncommitted changes.
    pub fn sha(&self) -> Option<&str> {
        match self {
            DiffSource::Commit { sha } | DiffSource::Stash { sha } => Some(sha),
            DiffSource::Unstaged | DiffSource::Staged => None,
        }
    }
}

/// Whether `source` is a side of the uncommitted changes.
pub fn is_uncommitted(source: &DiffSource) -> bool {
    matches!(source, DiffSource::Unstaged | DiffSource::Staged)
}

/// A file by its path in its source, e.g. to key what's kept of it: the same path can be both in
/// the staged and the unstaged changes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DiffFileKey {
    pub source: DiffSource,
    pub path: String,
}

impl DiffFileKey {
    pub fn new(source: &DiffSource, path: &str) -> Self {
        Self {
            source: source.clone(),
            path: path.to_owned(),
        }
    }
}

/// The file whose changes are on show: its path, in its source.
///
/// TS's `FileOpener` also opened and prefetched files, with callbacks the details passed down to
/// each list; in the app those are its own, and this is what its lists are told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShownFile {
    pub source: DiffSource,
    pub path: String,
}

/// The path of the file on show, if it's one of `source`'s, for its list to mark it.
pub fn shown_path_in<'a>(shown: Option<&'a ShownFile>, source: &DiffSource) -> Option<&'a str> {
    shown
        .filter(|shown| shown.source == *source)
        .map(|shown| shown.path.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(source: DiffSource, path: &str) -> ShownFile {
        ShownFile {
            source,
            path: path.into(),
        }
    }

    fn stash(sha: &str) -> DiffSource {
        DiffSource::Stash { sha: sha.into() }
    }

    mod diff_sources {
        use super::*;

        #[test]
        fn tell_the_same_path_in_the_staged_and_the_unstaged_changes_apart() {
            assert_ne!(
                DiffFileKey::new(&DiffSource::Staged, "a.txt"),
                DiffFileKey::new(&DiffSource::Unstaged, "a.txt")
            );
            assert_eq!(
                DiffFileKey::new(&stash("a1"), "a.txt"),
                DiffFileKey::new(&stash("a1"), "a.txt")
            );
            assert_ne!(
                DiffFileKey::new(&stash("a1"), "a.txt"),
                DiffFileKey::new(&DiffSource::Commit { sha: "a1".into() }, "a.txt")
            );
        }

        #[test]
        fn mark_the_file_on_show_only_in_the_list_its_from() {
            let shown = shown(DiffSource::Staged, "a.txt");
            assert_eq!(
                shown_path_in(Some(&shown), &DiffSource::Staged),
                Some("a.txt")
            );
            assert_eq!(shown_path_in(Some(&shown), &DiffSource::Unstaged), None);
            let in_a_stash = ShownFile {
                source: stash("a1"),
                ..shown.clone()
            };
            assert_eq!(
                shown_path_in(Some(&in_a_stash), &stash("a1")),
                Some("a.txt")
            );
            assert_eq!(shown_path_in(Some(&in_a_stash), &stash("b2")), None);
            assert_eq!(shown_path_in(None, &DiffSource::Staged), None);
        }
    }
}
