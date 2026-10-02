// Errors the git package throws. They describe what went wrong in git terms and carry a message a
// user can read; `toApiError` (in the middleware) decides how each one reaches the renderer.

/** A git command that failed, or couldn't start. */
export class GitError extends Error {
  constructor(
    message: string,
    readonly args: readonly string[],
    readonly exitCode: number | null,
    readonly stderr: string,
  ) {
    super(message);
    this.name = new.target.name;
  }
}

/** The repository's folder is no longer a git repository, e.g. its `.git` was deleted. */
export class NotARepositoryError extends GitError {}

/** Another git process holds `index.lock`, e.g. one running in the user's terminal. */
export class IndexLockedError extends GitError {}

/** HEAD isn't the commit that was meant to be amended, e.g. one was made in a terminal since. */
export class HeadMovedError extends Error {
  constructor(readonly expected: string) {
    super("The last commit has changed since you started amending it. Check it and try again.");
    this.name = new.target.name;
  }
}

/** No repository with that id has been added to Gitto. */
export class RepositoryNotFoundError extends Error {
  constructor(readonly repositoryId: string) {
    super("Repository not found.");
    this.name = new.target.name;
  }
}

/** The repository's folder has been moved or deleted. */
export class FolderNotFoundError extends Error {
  constructor(readonly path: string) {
    super(`${path} no longer exists.`);
    this.name = new.target.name;
  }
}

/** The most specific error for a command that exited with `exitCode`, going by what git printed. */
export function commandError(
  cwd: string,
  args: readonly string[],
  exitCode: number | null,
  stdout: string,
  stderr: string,
): GitError {
  if (/not a git repository/i.test(stderr)) {
    return new NotARepositoryError(`${cwd} is no longer a git repository.`, args, exitCode, stderr);
  }
  if (/index\.lock/.test(stderr)) {
    return new IndexLockedError(
      "Another git process is running in this repository.",
      args,
      exitCode,
      stderr,
    );
  }
  // Some failures, like "nothing to commit", are only explained on stdout.
  const message = stderr.trim() || stdout.trim() || `git ${args[0]} exited with code ${exitCode}`;
  return new GitError(message, args, exitCode, stderr);
}
