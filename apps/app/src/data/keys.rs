//! Query keys for git data. Ported from `git/queries/keys.ts`. Everything under
//! [`repository`]`(id)` is refetched when it changes on disk (see `super::watch`).

use gitto_git::input::RepositoryId;

use super::key::QueryKey;

/// Every repository's data that can change on disk; see `QueryClient`'s defaults for how it's kept
/// fresh.
pub fn all() -> QueryKey {
    QueryKey::new(["git"])
}

pub fn repository(repository_id: &RepositoryId) -> QueryKey {
    all().with(repository_id.to_string())
}

/// What's changed since the last commit: refetched on its own when files are edited or staged.
pub fn uncommitted(repository_id: &RepositoryId) -> QueryKey {
    repository(repository_id).with("uncommitted")
}

/// The status, with the changed files: one query, so the working tree is only walked once.
pub fn status(repository_id: &RepositoryId) -> QueryKey {
    uncommitted(repository_id).with("status")
}

/// Which side of the uncommitted changes a patch is of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UncommittedSide {
    Unstaged,
    Staged,
}

impl UncommittedSide {
    pub fn as_str(self) -> &'static str {
        match self {
            UncommittedSide::Unstaged => "unstaged",
            UncommittedSide::Staged => "staged",
        }
    }
}

/// The patch of a file's unstaged or staged changes, by its path.
pub fn uncommitted_file_patch(
    repository_id: &RepositoryId,
    side: UncommittedSide,
    path: &str,
) -> QueryKey {
    uncommitted(repository_id)
        .with("patch")
        .with(side.as_str())
        .with(path.to_owned())
}

/// A conflicted file's sides, and its text with the conflict markers, by its path.
pub fn conflict(repository_id: &RepositoryId, path: &str) -> QueryKey {
    uncommitted(repository_id)
        .with("conflict")
        .with(path.to_owned())
}

/// The merge, rebase, cherry-pick or revert under way: refetched when the refs change.
pub fn operation(repository_id: &RepositoryId) -> QueryKey {
    repository(repository_id).with("operation")
}

pub fn log(repository_id: &RepositoryId) -> QueryKey {
    repository(repository_id).with("log")
}

pub fn refs(repository_id: &RepositoryId) -> QueryKey {
    repository(repository_id).with("refs")
}

/// The remotes' names: refetched when the refs change, as the config does.
pub fn remotes(repository_id: &RepositoryId) -> QueryKey {
    repository(repository_id).with("remotes")
}

pub fn stashes(repository_id: &RepositoryId) -> QueryKey {
    repository(repository_id).with("stashes")
}

/// The worktrees: refetched when the refs change, as the git directory's `worktrees` folder does.
pub fn worktrees(repository_id: &RepositoryId) -> QueryKey {
    repository(repository_id).with("worktrees")
}

/// What the home page shows of a repository, beyond its status.
pub fn overview(repository_id: &RepositoryId) -> QueryKey {
    repository(repository_id).with("overview")
}

/// Which remote branch has a commit: refetched when the refs change, e.g. after a push.
pub fn pushed_to(repository_id: &RepositoryId, sha: &str) -> QueryKey {
    repository(repository_id)
        .with("pushed-to")
        .with(sha.to_owned())
}

/// Where the Push button pushes, and how many commits: refetched when the refs or config change.
pub fn push_target(repository_id: &RepositoryId) -> QueryKey {
    repository(repository_id).with("push-target")
}

/// A commit never changes, so it and its files are kept outside [`repository`]`(id)`.
pub fn commit(repository_id: &RepositoryId, sha: &str) -> QueryKey {
    QueryKey::new([
        "git-commit".to_owned(),
        repository_id.to_string(),
        sha.to_owned(),
    ])
}

pub fn commit_files(repository_id: &RepositoryId, sha: &str) -> QueryKey {
    commit(repository_id, sha).with("files")
}

pub fn commit_message(repository_id: &RepositoryId, sha: &str) -> QueryKey {
    commit(repository_id, sha).with("message")
}

/// The patch of one of a commit's files, by its path.
pub fn commit_file_patch(repository_id: &RepositoryId, sha: &str, path: &str) -> QueryKey {
    commit(repository_id, sha)
        .with("patch")
        .with(path.to_owned())
}

/// A file's contents by their object name, which never change.
pub fn blob(repository_id: &RepositoryId, oid: &str) -> QueryKey {
    QueryKey::new([
        "git-blob".to_owned(),
        repository_id.to_string(),
        oid.to_owned(),
    ])
}

/// A stash is a commit too, and never changes either.
pub fn stash_files(repository_id: &RepositoryId, sha: &str) -> QueryKey {
    commit(repository_id, sha).with("stash-files")
}

/// The patch of one of a stash's files, by its path.
pub fn stash_file_patch(repository_id: &RepositoryId, sha: &str, path: &str) -> QueryKey {
    commit(repository_id, sha)
        .with("stash-patch")
        .with(path.to_owned())
}

/// The installed git, and whether Gitto works with it (`GIT_VERSION_KEY` in `version.ts`).
pub fn git_version() -> QueryKey {
    QueryKey::new(["git-version"])
}

/// The name the user commits with (`["git-user-name"]` in `user.ts`).
pub fn git_user_name() -> QueryKey {
    QueryKey::new(["git-user-name"])
}

/// The repositories added to Gitto (`REPOSITORIES_KEY` in `hooks/repositories.ts`).
pub fn repositories() -> QueryKey {
    QueryKey::new(["repositories"])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id() -> RepositoryId {
        RepositoryId::parse("01890a5d-ac96-774b-bcce-b302099a8057").unwrap()
    }

    #[test]
    fn keeps_what_changes_on_disk_under_the_repository() {
        let id = &id();
        for key in [
            status(id),
            uncommitted_file_patch(id, UncommittedSide::Staged, "a.txt"),
            conflict(id, "a.txt"),
            operation(id),
            log(id),
            refs(id),
            remotes(id),
            stashes(id),
            worktrees(id),
            overview(id),
            pushed_to(id, "abc"),
            push_target(id),
        ] {
            assert!(key.starts_with(&repository(id)), "{key:?}");
        }
        assert!(status(id).starts_with(&uncommitted(id)));
        assert!(!operation(id).starts_with(&uncommitted(id)));
    }

    #[test]
    fn keeps_what_never_changes_outside_it() {
        let id = &id();
        for key in [
            commit(id, "abc"),
            commit_files(id, "abc"),
            commit_file_patch(id, "abc", "a.txt"),
            blob(id, "def"),
            stash_files(id, "abc"),
        ] {
            assert!(!key.starts_with(&all()), "{key:?}");
        }
        assert_eq!(
            commit_message(id, "abc"),
            QueryKey::new([
                "git-commit",
                "01890a5d-ac96-774b-bcce-b302099a8057",
                "abc",
                "message"
            ])
        );
    }
}
