// Errors the git package throws. They describe what went wrong in git terms and carry a message a
// user can read; `toApiError` (in the middleware) decides how each one reaches the renderer.

/** A git command that failed, or couldn't start. */
export class GitError extends Error {
  constructor(
    message: string,
    readonly args: readonly string[],
    readonly exitCode: number | null,
    readonly stderr: string,
    /** What it printed on stdout, for a command whose output still says something when it fails. */
    readonly stdout = "",
  ) {
    super(message);
    this.name = new.target.name;
  }

  /** This failure worded as `message`; of the same class, so it reaches the renderer the same way. */
  withMessage(message: string): GitError {
    const Class = this.constructor as typeof GitError;
    return new Class(message, this.args, this.exitCode, this.stderr, this.stdout);
  }

  /** This failure without git's hints, which suggest commands to type and so don't help in the app. */
  withoutHints(): GitError {
    const message = this.message
      .split("\n")
      .filter((line) => !line.startsWith("hint:"))
      .join("\n")
      .trim();
    return !message || message === this.message ? this : this.withMessage(message);
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

/** A file's changes, too many to send whole: an untracked log file of hundreds of megabytes, say. */
export class ChangesTooLargeError extends MessageError {
  constructor(bytes: number) {
    super(`These changes are over ${(bytes / 1024 / 1024).toFixed(0)} MB, too large to show.`);
  }
}

/** A path that isn't a file inside the repository's working tree, e.g. with `..`, or a link out. */
export class OutsideRepositoryError extends MessageError {
  constructor(path: string) {
    super(`${path} isn't a file in this repository.`);
  }
}

/** A file that's no longer in the working tree, e.g. deleted since its changes were read. */
export class WorkingTreeFileNotFoundError extends MessageError {
  constructor(path: string) {
    super(`${path} is no longer in the working tree.`);
  }
}

/** A file whose contents aren't UTF-8, which can't be shown or edited as text. */
export class NotUtf8Error extends MessageError {
  constructor() {
    super("This file isn't UTF-8 text.");
  }
}

/**
 * A file in the working tree that changed on disk since it was read to be edited, which saving
 * the edits over would lose.
 */
export class FileChangedOnDiskError extends MessageError {
  constructor(path: string, how: "changed" | "deleted" = "changed") {
    super(`${path} was ${how} on disk since you started editing it.`);
  }
}

/**
 * A file's changes that aren't what the user picked lines from any more, e.g. as the file was
 * saved since: staging or unstaging lines of them would move others than the ones they saw.
 */
export class PatchChangedError extends MessageError {
  constructor(action: "staged" | "unstaged") {
    super(`The file changed since its changes were shown, so nothing was ${action}.`);
  }
}

/** Lines that can't be staged or unstaged on their own, e.g. of a binary file. */
export class LinesNotStageableError extends MessageError {}

/**
 * Conflicted files that still have conflict markers, which staging would mark resolved with the
 * markers in them.
 */
export class ConflictMarkersError extends MessageError {
  constructor(readonly paths: string[]) {
    const [first] = paths;
    const others = paths.length - 1;
    super(
      others === 0
        ? `${first} still has conflict markers. Resolve its conflicts, or mark it resolved with them from its changes if they belong in it.`
        : `${first} and ${others} other ${others === 1 ? "file" : "files"} still have conflict markers. Resolve their conflicts, or mark each resolved with them from its changes if they belong in it.`,
    );
  }
}

/** A conflict that changed since it was shown, e.g. resolved in a terminal: nothing was done. */
export class ConflictChangedError extends MessageError {
  constructor(path: string) {
    super(`The conflict in ${path} changed since it was shown, so nothing was resolved.`);
  }
}

/** No merge, rebase, cherry-pick, revert or `git am` is under way, or another one than shown. */
export class NoOperationError extends MessageError {}

/**
 * A merge that can't start: HEAD isn't on a branch, an operation or conflicts are under way, or
 * the branch to merge is gone. Nothing was done.
 */
export class MergeBlockedError extends MessageError {}

/**
 * A merge that stopped before committing, without conflicts, e.g. as a hook turned the merge commit
 * down: it's under way, to commit or abort.
 */
export class MergeStoppedError extends MessageError {}

/** Changes that can't be discarded, as they're of conflicted files: nothing was discarded. */
export class DiscardBlockedError extends MessageError {}

/**
 * A worktree that can't be removed: the main one, which the others share their git directory
 * with, or the one open here. Nothing was done.
 */
export class WorktreeRemovalBlockedError extends MessageError {}

/** What's asked of git needs an editor for a message, which Gitto can't open: a rebase's reword. */
export class EditorNeededError extends MessageError {}

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
  return new GitError(message, args, exitCode, stderr, stdout);
}
