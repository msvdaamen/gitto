/** Query keys for git data. Everything under `repository(id)` is refetched when it changes on disk. */
export const gitKeys = {
  repository: (repositoryId: string) => ["git", repositoryId] as const,
  status: (repositoryId: string) => [...gitKeys.repository(repositoryId), "status"] as const,
  log: (repositoryId: string) => [...gitKeys.repository(repositoryId), "log"] as const,
  refs: (repositoryId: string) => [...gitKeys.repository(repositoryId), "refs"] as const,
  workingTreeFiles: (repositoryId: string) =>
    [...gitKeys.repository(repositoryId), "workingTreeFiles"] as const,
  /** A commit never changes, so it and its files are kept outside `repository(id)`. */
  commit: (repositoryId: string, sha: string) => ["git-commit", repositoryId, sha] as const,
  commitFiles: (repositoryId: string, sha: string) =>
    [...gitKeys.commit(repositoryId, sha), "files"] as const,
  /** Avatars are looked up online, so they're kept outside `repository(id)` too. */
  avatar: (repositoryId: string, email: string) =>
    ["git-avatar", repositoryId, email.trim().toLowerCase()] as const,
};
