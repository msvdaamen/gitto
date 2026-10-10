/** Query keys for git data. Everything under `repository(id)` is refetched when it changes on disk. */
export const gitKeys = {
  /** Every repository's data that can change on disk; see `queryClient` for how it's kept fresh. */
  all: ["git"] as const,
  repository: (repositoryId: string) => [...gitKeys.all, repositoryId] as const,
  /** What's changed since the last commit: refetched on its own when files are edited or staged. */
  uncommitted: (repositoryId: string) =>
    [...gitKeys.repository(repositoryId), "uncommitted"] as const,
  /** The status, with the changed files: one query, so the working tree is only walked once. */
  status: (repositoryId: string) => [...gitKeys.uncommitted(repositoryId), "status"] as const,
  /** The patch of a file's unstaged or staged changes, by its path. */
  uncommittedFilePatch: (repositoryId: string, side: "unstaged" | "staged", path: string) =>
    [...gitKeys.uncommitted(repositoryId), "patch", side, path] as const,
  /** A conflicted file's sides, and its text with the conflict markers, by its path. */
  conflict: (repositoryId: string, path: string) =>
    [...gitKeys.uncommitted(repositoryId), "conflict", path] as const,
  /** The merge, rebase, cherry-pick or revert under way: refetched when the refs change. */
  operation: (repositoryId: string) => [...gitKeys.repository(repositoryId), "operation"] as const,
  log: (repositoryId: string) => [...gitKeys.repository(repositoryId), "log"] as const,
  refs: (repositoryId: string) => [...gitKeys.repository(repositoryId), "refs"] as const,
  stashes: (repositoryId: string) => [...gitKeys.repository(repositoryId), "stashes"] as const,
  /** The worktrees: refetched when the refs change, as the git directory's `worktrees` folder does. */
  worktrees: (repositoryId: string) => [...gitKeys.repository(repositoryId), "worktrees"] as const,
  /** What the home page shows of a repository, beyond its status. */
  overview: (repositoryId: string) => [...gitKeys.repository(repositoryId), "overview"] as const,
  /** Which remote branch has a commit: refetched when the refs change, e.g. after a push. */
  pushedTo: (repositoryId: string, sha: string) =>
    [...gitKeys.repository(repositoryId), "pushed-to", sha] as const,
  /** A commit never changes, so it and its files are kept outside `repository(id)`. */
  commit: (repositoryId: string, sha: string) => ["git-commit", repositoryId, sha] as const,
  commitFiles: (repositoryId: string, sha: string) =>
    [...gitKeys.commit(repositoryId, sha), "files"] as const,
  commitMessage: (repositoryId: string, sha: string) =>
    [...gitKeys.commit(repositoryId, sha), "message"] as const,
  /** The patch of one of a commit's files, by its path. */
  commitFilePatch: (repositoryId: string, sha: string, path: string) =>
    [...gitKeys.commit(repositoryId, sha), "patch", path] as const,
  /** A file's contents by their object name, which never change. */
  blob: (repositoryId: string, oid: string) => ["git-blob", repositoryId, oid] as const,
  /** A stash is a commit too, and never changes either. */
  stashFiles: (repositoryId: string, sha: string) =>
    [...gitKeys.commit(repositoryId, sha), "stash-files"] as const,
  /** The patch of one of a stash's files, by its path. */
  stashFilePatch: (repositoryId: string, sha: string, path: string) =>
    [...gitKeys.commit(repositoryId, sha), "stash-patch", path] as const,
};
