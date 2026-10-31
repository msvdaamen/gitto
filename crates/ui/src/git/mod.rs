//! What the UI makes of what gitto-git gives it: ported from `apps/ui/src/git/*.ts`, one module
//! per file (`ref-labels.ts` → [`ref_labels`]). `git/queries/`, which fetched it, is the app's.

pub mod activity;
pub mod changes;
pub mod conflicts;
pub mod diff_source;
pub mod graph;
pub mod message;
pub mod patch;
pub mod push;
pub mod ref_labels;
pub mod ref_tree;
pub mod remote;
pub mod rows;
pub mod search;
pub mod stash;
pub mod status;
pub mod worktree;

/// A commit's SHA shortened, as the UI shows one: its first 7 characters (the whole of a shorter
/// one), as TS's `sha.slice(0, 7)` did.
pub fn short_sha(sha: &str) -> &str {
    sha.char_indices()
        .nth(7)
        .map_or(sha, |(end, _)| &sha[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_sha_keeps_the_first_seven_characters() {
        assert_eq!(short_sha("1a2b3c4d5e6f"), "1a2b3c4");
        assert_eq!(short_sha("1a2b"), "1a2b");
    }
}
