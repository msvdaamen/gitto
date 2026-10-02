/** Query keys for git data. Everything under `repository(id)` is refetched when it changes on disk. */
export const gitKeys = {
  repository: (repositoryId: string) => ["git", repositoryId] as const,
  /** What's changed since the last commit: refetched on its own when files are edited or staged. */
  uncommitted: (repositoryId: string) =>
    [...gitKeys.repository(repositoryId), "uncommitted"] as const,
  /** The status, with the changed files: one query, so the working tree is only walked once. */
  status: (repositoryId: string) => [...gitKeys.uncommitted(repositoryId), "status"] as const,
  /** Line counts of the uncommitted changes to `paths`, on one side of the index. */
  lineCounts: (repositoryId: string, side: "staged" | "unstaged", paths: string[]) =>
    [...gitKeys.uncommitted(repositoryId), "lines", side, paths] as const,
  log: (repositoryId: string) => [...gitKeys.repository(repositoryId), "log"] as const,
  refs: (repositoryId: string) => [...gitKeys.repository(repositoryId), "refs"] as const,
  /** A commit never changes, so it and its files are kept outside `repository(id)`. */
  commit: (repositoryId: string, sha: string) => ["git-commit", repositoryId, sha] as const,
  commitFiles: (repositoryId: string, sha: string) =>
    [...gitKeys.commit(repositoryId, sha), "files"] as const,
};
