// Errors the repository package throws; `toApiError` (in the handler) decides how each one reaches
// the renderer.

/** The folder someone tried to add isn't a git repository. */
export class NotAGitRepositoryError extends Error {
  constructor(readonly path: string) {
    super(`${path} is not a git repository.`);
    this.name = new.target.name;
  }
}

/**
 * Gitto's list of repositories couldn't be changed, e.g. as its database is locked or the disk is
 * full: said with SQLite's reason, which the database's own error wraps in its query.
 */
export class RepositoryListError extends Error {
  constructor(cause: unknown) {
    super(`Gitto couldn't update its list of repositories: ${reason(cause)}`, { cause });
    this.name = new.target.name;
  }
}

/** The innermost error's message, with a full stop. */
function reason(error: unknown): string {
  let inner = error;
  while (inner instanceof Error && inner.cause instanceof Error) inner = inner.cause;
  const message = (inner instanceof Error ? inner.message : String(inner)).trim();
  return /[.!?]$/.test(message) ? message : `${message}.`;
}
