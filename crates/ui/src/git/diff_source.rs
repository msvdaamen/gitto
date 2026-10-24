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
    /// As TS spells its `kind`, e.g. `unstaged`.
    pub fn kind(&self) -> &'static str {
        match self {
            DiffSource::Commit { .. } => "commit",
            DiffSource::Stash { .. } => "stash",
            DiffSource::Unstaged => "unstaged",
            DiffSource::Staged => "staged",
        }
    }

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

/// Names a file in `source`, to tell whether it's the one on show: the same path can be both in the
/// staged and the unstaged changes.
pub fn diff_file_key(source: &DiffSource, path: &str) -> String {
    format!("{}:{}:{path}", source.kind(), source.sha().unwrap_or(""))
}

/// Whether `a` and `b` are the same commit, stash, or side of the uncommitted changes.
pub fn is_same_source(a: &DiffSource, b: &DiffSource) -> bool {
    a == b
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
        .filter(|shown| is_same_source(&shown.source, source))
        .map(|shown| shown.path.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opener(source: DiffSource, path: &str) -> ShownFile {
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
                diff_file_key(&DiffSource::Staged, "a.txt"),
                diff_file_key(&DiffSource::Unstaged, "a.txt")
            );
            assert!(is_same_source(&stash("a1"), &stash("a1")));
            assert!(!is_same_source(
                &stash("a1"),
                &DiffSource::Commit { sha: "a1".into() }
            ));
        }

        #[test]
        fn mark_the_file_on_show_only_in_the_list_its_from() {
            let files = opener(DiffSource::Staged, "a.txt");
            assert_eq!(
                shown_path_in(Some(&files), &DiffSource::Staged),
                Some("a.txt")
            );
            assert_eq!(shown_path_in(Some(&files), &DiffSource::Unstaged), None);
            assert_eq!(shown_path_in(None, &DiffSource::Staged), None);
        }
    }

    #[test]
    fn keys_files_as_ts_did() {
        assert_eq!(diff_file_key(&stash("a1"), "b.txt"), "stash:a1:b.txt");
        assert_eq!(diff_file_key(&DiffSource::Staged, "b.txt"), "staged::b.txt");
    }
}
