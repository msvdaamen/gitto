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

/** An error that's only a message for the user; `name` is its class's, as for `GitError`. */
class MessageError extends Error {
  constructor(message: string) {
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
 * user to finish. Not a `GitError`: git may well have succeeded, e.g. stopping a merge before
 * committing because it was told to.
 */
export class PullInterruptedError extends MessageError {}

/** A stash that was popped but conflicts, left for the user to resolve; the stash is kept. */
export class StashConflictError extends MessageError {}

/**
 * Uncommitted changes that were left in the stash rather than brought along to the branch switched
 * to: they conflict with it, or stashing them failed partway.
 */
export class ChangesStashedError extends MessageError {}

/** There's nothing to pull from: HEAD isn't on a branch, or the branch doesn't track one. */
export class NoUpstreamError extends MessageError {}

/** Git isn't installed, or is older than Gitto works with (see `GitVersion`). */
export class UnsupportedGitError extends MessageError {
  constructor(install: { version: string | null; required: string }) {
    super(
      install.version === null
        ? `Gitto couldn't find Git. Install Git ${install.required} or newer.`
        : `Gitto needs Git ${install.required} or newer, but Git ${install.version} is installed.`,
    );
  }
}

/** The repository changed while an operation was under way, which trying again will get past. */
export class RepositoryChangedError extends MessageError {}

/** HEAD isn't the commit that was meant to be amended, e.g. one was made in a terminal since. */
export class HeadMovedError extends MessageError {
  constructor() {
    super("The last commit changed before it could be amended, so nothing was amended.");
  }
}

/** A file too large to be read whole, e.g. to show more of it around its changes. */
export class FileTooLargeError extends MessageError {
  constructor(bytes: number) {
    super(`This file is ${(bytes / 1024 / 1024).toFixed(1)} MB, too large to show more of it.`);
  }
}

/** No repository with that id has been added to Gitto. */
export class RepositoryNotFoundError extends MessageError {
  constructor(readonly repositoryId: string) {
    super("Repository not found.");
  }
}

/** The repository's folder has been moved or deleted. */
export class FolderNotFoundError extends MessageError {
  constructor(readonly path: string) {
    super(`${path} no longer exists.`);
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
