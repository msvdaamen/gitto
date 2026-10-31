//! What a change in the git directory means for the UI. Ported from `git-dirs.ts`.

use std::path::{Component, Path};
use std::sync::LazyLock;

use regex::Regex;

use super::schema::GitDirChange;
use crate::core::git_dirs::GitDirs;

/// Files in a worktree's own git directory: HEAD (and MERGE_HEAD etc.), operations in progress,
/// and its refs when they're stored in a reftable (`git init --ref-format=reftable`).
static WORKTREE_STATE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^([A-Z_]*HEAD|(rebase-merge|rebase-apply|sequencer|reftable)(/.*)?)$").unwrap()
});

/// Files in the shared git directory: branches, tags and remotes (as files, packed, or in a
/// reftable), the config (upstreams), and the other worktrees: each one's folder, which comes and
/// goes with it, its HEAD, where its folder is, and whether it's locked; not its index.
static SHARED_STATE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^((refs|reftable)(/.*)?|packed-refs|config|worktrees(/[^/]+(/(HEAD|gitdir|locked))?)?)$",
    )
    .unwrap()
});

/// What a change to `path`, in the git directory, means for the UI; `None` if nothing.
pub fn classify(dirs: &GitDirs, path: &Path) -> Option<GitDirChange> {
    // Written to while git works, then renamed onto the real file; that's the change that counts.
    if path.as_os_str().as_encoded_bytes().ends_with(b".lock") {
        return None;
    }

    let own = inside(&dirs.git_dir, path);
    if own.as_deref() == Some("index") {
        return Some(GitDirChange::Index);
    }
    if own.is_some_and(|own| WORKTREE_STATE.is_match(&own)) {
        return Some(GitDirChange::Refs);
    }

    let shared = inside(&dirs.common_dir, path);
    if shared.is_some_and(|shared| SHARED_STATE.is_match(&shared)) {
        return Some(GitDirChange::Refs);
    }
    None
}

/// `path` relative to `dir` and `/`-separated, or `None` if it's not inside `dir` (or is `dir`).
pub fn inside(dir: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(dir).ok()?;
    let parts: Vec<_> = rel
        .components()
        .map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy()),
            _ => None,
        })
        .collect::<Option<_>>()?;
    if parts.is_empty() {
        return None;
    }
    Some(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn root() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(r"C:\")
        } else {
            PathBuf::from("/")
        }
    }

    fn dirs() -> GitDirs {
        let common = root().join("repo").join(".git");
        GitDirs {
            git_dir: common.join("worktrees").join("wt"),
            exclude_file: common.join("info").join("exclude"),
            common_dir: common,
        }
    }

    /// A path in `dir`, with the platform's separators.
    fn at(dir: &Path, parts: &[&str]) -> PathBuf {
        parts
            .iter()
            .fold(dir.to_owned(), |path, part| path.join(part))
    }

    use GitDirChange::{Index, Refs};

    #[test]
    fn takes_the_index_head_and_operations_in_progress_for_the_worktrees_own() {
        let dirs = dirs();
        let own = &dirs.git_dir;
        assert_eq!(classify(&dirs, &at(own, &["index"])), Some(Index));
        assert_eq!(classify(&dirs, &at(own, &["HEAD"])), Some(Refs));
        assert_eq!(classify(&dirs, &at(own, &["MERGE_HEAD"])), Some(Refs));
        assert_eq!(
            classify(&dirs, &at(own, &["rebase-merge", "done"])),
            Some(Refs)
        );
        assert_eq!(classify(&dirs, &at(own, &["sequencer"])), Some(Refs));
    }

    #[test]
    fn takes_the_refs_packed_refs_reftable_and_config_for_the_shared_ones() {
        let dirs = dirs();
        let shared = &dirs.common_dir;
        assert_eq!(
            classify(&dirs, &at(shared, &["refs", "heads", "main"])),
            Some(Refs)
        );
        assert_eq!(classify(&dirs, &at(shared, &["packed-refs"])), Some(Refs));
        assert_eq!(
            classify(&dirs, &at(shared, &["reftable", "tables.list"])),
            Some(Refs)
        );
        assert_eq!(classify(&dirs, &at(shared, &["config"])), Some(Refs));
    }

    #[test]
    fn takes_the_other_worktrees_folders_heads_locks_and_gitdir_files_for_the_shared_ones() {
        let dirs = dirs();
        let shared = &dirs.common_dir;
        assert_eq!(classify(&dirs, &at(shared, &["worktrees"])), Some(Refs));
        assert_eq!(
            classify(&dirs, &at(shared, &["worktrees", "other"])),
            Some(Refs)
        );
        assert_eq!(
            classify(&dirs, &at(shared, &["worktrees", "other", "HEAD"])),
            Some(Refs)
        );
        assert_eq!(
            classify(&dirs, &at(shared, &["worktrees", "other", "gitdir"])),
            Some(Refs)
        );
        assert_eq!(
            classify(&dirs, &at(shared, &["worktrees", "other", "locked"])),
            Some(Refs)
        );
        // Another worktree's index, and what it's doing, aren't shown.
        assert_eq!(
            classify(&dirs, &at(shared, &["worktrees", "other", "index"])),
            None
        );
        assert_eq!(
            classify(&dirs, &at(shared, &["worktrees", "other", "MERGE_HEAD"])),
            None
        );
        assert_eq!(
            classify(
                &dirs,
                &at(shared, &["worktrees", "other", "rebase-merge", "done"])
            ),
            None
        );
        // This worktree's own index is its own.
        assert_eq!(classify(&dirs, &at(&dirs.git_dir, &["index"])), Some(Index));
    }

    #[test]
    fn ignores_lock_files_objects_logs_and_files_outside_both_directories() {
        let dirs = dirs();
        assert_eq!(classify(&dirs, &at(&dirs.git_dir, &["index.lock"])), None);
        assert_eq!(
            classify(
                &dirs,
                &at(&dirs.common_dir, &["refs", "heads", "main.lock"])
            ),
            None
        );
        assert_eq!(
            classify(&dirs, &at(&dirs.common_dir, &["objects", "ab", "cd"])),
            None
        );
        assert_eq!(
            classify(&dirs, &at(&dirs.common_dir, &["logs", "HEAD"])),
            None
        );
        assert_eq!(
            classify(&dirs, &at(&root(), &["repo", "src", "HEAD"])),
            None
        );
    }

    #[test]
    fn takes_the_shared_directorys_head_for_the_main_worktrees_when_theyre_the_same() {
        let main = GitDirs {
            git_dir: dirs().common_dir,
            ..dirs()
        };
        assert_eq!(classify(&main, &at(&main.git_dir, &["HEAD"])), Some(Refs));
        assert_eq!(classify(&main, &at(&main.git_dir, &["index"])), Some(Index));
    }

    #[test]
    fn inside_is_the_path_relative_to_the_directory_with_forward_slashes() {
        let dirs = dirs();
        assert_eq!(
            inside(
                &dirs.common_dir,
                &at(&dirs.common_dir, &["refs", "heads", "main"])
            )
            .as_deref(),
            Some("refs/heads/main")
        );
    }

    #[test]
    fn inside_is_none_for_the_directory_itself_its_parents_and_paths_beside_it() {
        let dirs = dirs();
        assert_eq!(inside(&dirs.common_dir, &dirs.common_dir), None);
        assert_eq!(inside(&dirs.common_dir, &at(&root(), &["repo"])), None);
        assert_eq!(
            inside(&dirs.common_dir, &at(&root(), &["repo", ".github", "x"])),
            None
        );
    }
}
