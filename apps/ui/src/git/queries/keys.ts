/** Query keys for git data. Everything under `repository(id)` is refetched when it changes on disk. */
export const gitKeys = {
  repository: (repositoryId: string) => ["git", repositoryId] as const,
  /** What's changed since the last commit: refetched on its own when files are edited or staged. */
  uncommitted: (repositoryId: string) =>
    [...gitKeys.repository(repositoryId), "uncommitted"] as const,
  /** The status, with the changed files: one query, so the working tree is only walked once. */
  status: (repositoryId: string) => [...gitKeys.uncommitted(repositoryId), "status"] as const,
  log: (repositoryId: string) => [...gitKeys.repository(repositoryId), "log"] as const,
  refs: (repositoryId: string) => [...gitKeys.repository(repositoryId), "refs"] as const,
  stashes: (repositoryId: string) => [...gitKeys.repository(repositoryId), "stashes"] as const,
  /** Which remote branch has a commit: refetched when the refs change, e.g. after a push. */
  pushedTo: (repositoryId: string, sha: string) =>
    [...gitKeys.repository(repositoryId), "pushed-to", sha] as const,
  /** A commit never changes, so it and its files are kept outside `repository(id)`. */
  commit: (repositoryId: string, sha: string) => ["git-commit", repositoryId, sha] as const,
  commitFiles: (repositoryId: string, sha: string) =>
    [...gitKeys.commit(repositoryId, sha), "files"] as const,
  commitMessage: (repositoryId: string, sha: string) =>
    [...gitKeys.commit(repositoryId, sha), "message"] as const,
  /** A stash is a commit too, and never changes either. */
  stashFiles: (repositoryId: string, sha: string) =>
    [...gitKeys.commit(repositoryId, sha), "stash-files"] as const,
};
