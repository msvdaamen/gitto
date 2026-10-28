//! Ported from `git/search.ts`: the history's search.

use super::ref_labels::ref_label_matches;
use super::rows::{HistoryRow, WIP_MESSAGE};

/// Lowercases and trims what was typed in the history search; empty means no search.
pub fn search_needle(search: &str) -> String {
    search.trim().to_lowercase()
}

/// Whether a history row matches `needle` (see [`search_needle`]): a commit by its message, author,
/// SHA or one of its branches and tags; a stash by its message or SHA; the uncommitted changes by
/// what their row says.
pub fn matches_search(row: &HistoryRow, needle: &str) -> bool {
    match row {
        HistoryRow::Wip(_) => contains_lowercase(WIP_MESSAGE.chars(), needle),
        HistoryRow::Stash(row) => contains_lowercase(
            row.message.chars().chain([' ']).chain(row.sha.chars()),
            needle,
        ),
        HistoryRow::Commit(row) => {
            let text = row.message.chars().chain([' ']).chain(row.author.chars());
            contains_lowercase(text.chain([' ']).chain(row.id.chars()), needle)
                || row
                    .labels
                    .iter()
                    .any(|label| ref_label_matches(label, needle))
        }
    }
}

/// Whether the text `haystack`, lowercased, contains `needle`, which is lowercase. The text isn't
/// lowercased into a string of its own: the search runs on every row at each keystroke.
pub(crate) fn contains_lowercase<I>(haystack: I, needle: &str) -> bool
where
    I: Iterator<Item = char> + Clone,
{
    let mut rest = haystack.flat_map(char::to_lowercase);
    loop {
        let mut candidate = rest.clone();
        if needle.chars().all(|c| candidate.next() == Some(c)) {
            return true;
        }
        if rest.next().is_none() {
            return false;
        }
    }
}

#[cfg(test)]
mod tests {
    use gitto_git::input::RepositoryId;

    use super::*;
    use crate::git::ref_labels::{BranchLabel, RefLabel};
    use crate::git::rows::{CommitRow, StashRow, WipRow};

    fn commit() -> HistoryRow {
        HistoryRow::Commit(CommitRow {
            id: "a1b2c3d4e5".into(),
            repository_id: RepositoryId::generate(),
            short_sha: "a1b2c3d".into(),
            graph: None,
            message: "Fix the parser".into(),
            description: None,
            author: "Ada Lovelace".into(),
            initials: "AL".into(),
            avatar_color: 0x000000,
            committed_at: 0,
            labels: vec![RefLabel::Branch(BranchLabel {
                name: "main".into(),
                full_name: "refs/heads/main".into(),
                local: true,
                remotes: vec!["origin".into()],
                current: true,
                worktree: None,
            })],
        })
    }

    fn stash() -> HistoryRow {
        HistoryRow::Stash(StashRow {
            sha: "f00dfeed".into(),
            repository_id: RepositoryId::generate(),
            graph: None,
            message: "WIP on main: a1b2c3d Fix the parser".into(),
            created_at: 0,
        })
    }

    fn wip() -> HistoryRow {
        HistoryRow::Wip(WipRow {
            repository_id: RepositoryId::generate(),
            graph: None,
        })
    }

    mod matches_search {
        use super::*;

        #[test]
        fn matches_a_commit_by_its_message_author_sha_or_labels() {
            let commit = commit();
            assert!(matches_search(&commit, &search_needle(" PARSER ")));
            assert!(matches_search(&commit, &search_needle("lovelace")));
            assert!(matches_search(&commit, &search_needle("a1b2")));
            assert!(matches_search(&commit, &search_needle("origin/main")));
            assert!(!matches_search(&commit, &search_needle("develop")));
        }

        #[test]
        fn matches_a_stash_by_its_message_or_sha() {
            let stash = stash();
            assert!(matches_search(&stash, &search_needle("wip on main")));
            assert!(matches_search(&stash, &search_needle("f00d")));
            assert!(!matches_search(&stash, &search_needle("stash:")));
        }

        #[test]
        fn matches_the_uncommitted_changes_by_what_their_row_says() {
            let wip = wip();
            assert!(matches_search(&wip, &search_needle("uncommitted")));
            assert!(!matches_search(&wip, &search_needle("parser")));
        }
    }

    #[test]
    fn finds_lowercase_text_in_any_case_as_lowercasing_it_would() {
        assert!(contains_lowercase("Fix the Parser".chars(), "the parser"));
        assert!(contains_lowercase("ÉMILE".chars(), "émile"));
        // `İ` lowercases to two characters, `i` and a combining dot.
        assert!(contains_lowercase("İX".chars(), "\u{307}x"));
        assert!(contains_lowercase("abc".chars(), ""));
        assert!(contains_lowercase("".chars(), ""));
        assert!(!contains_lowercase("".chars(), "a"));
        assert!(!contains_lowercase("Parse".chars(), "parser"));
    }
}
