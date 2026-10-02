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

/**
 * A pull that stopped partway, at conflicts or a rebase that couldn't go on, and is left for the
 * user to finish.
 */
export class PullInterruptedError extends GitError {}

/** There's nothing to pull from: HEAD isn't on a branch, or the branch doesn't track one. */
export class NoUpstreamError extends Error {
  constructor(message: string) {
    super(message);
    this.name = new.target.name;
  }
}

/** The repository changed while an operation was under way, which trying again will get past. */
export class RepositoryChangedError extends Error {
  constructor(message: string) {
    super(message);
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
  const command = args.find((arg, i) => !arg.startsWith("-") && args[i - 1] !== "-c");
  const message = stderr.trim() || stdout.trim() || `git ${command} exited with code ${exitCode}`;
  return new GitError(message, args, exitCode, stderr);
}
