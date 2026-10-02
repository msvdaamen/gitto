// Errors the repository package throws; `toApiError` (in the handler) decides how each one reaches
// the renderer.

/** The folder someone tried to add isn't a git repository. */
export class NotAGitRepositoryError extends Error {
  constructor(readonly path: string) {
    super(`${path} is not a git repository.`);
    this.name = new.target.name;
  }
}
