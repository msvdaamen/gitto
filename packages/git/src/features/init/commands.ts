import { stat } from "node:fs/promises";
import { homedir } from "node:os";
import { join } from "node:path";

import { GitError, InitFailedError } from "../../core/errors";
import { fullPath } from "../../core/paths";
import { runGit } from "../../core/runner";

/** The branch a new repository starts on, unless the user's `init.defaultBranch` says another. */
export const DEFAULT_BRANCH = "main";

/**
 * Creates an empty repository in `folder`, its full path or one starting with `~` for the home
 * folder, and returns that path. The folder is made if it isn't there; if it is, what's in it is
 * kept, untracked. It starts on the branch the user's `init.defaultBranch` names, or `main`.
 */
export async function initRepository(folder: string): Promise<string> {
  const path = fullPath(folder);
  if (!path) {
    throw new InitFailedError(
      `${folder.trim()} isn't a full path. Choose the folder to create it in, or type its full path.`,
    );
  }
  // Git says only `cannot mkdir …: File exists`.
  if ((await stat(path).catch(() => undefined))?.isDirectory() === false) {
    throw new InitFailedError(`${path} is a file. Choose a folder, or a name that isn't taken.`);
  }
  // `.git` is a folder in a repository, and a file in a worktree or submodule. Git would
  // reinitialize either, quietly, rather than say it's there.
  if (await stat(join(path, ".git")).catch(() => undefined)) {
    throw new InitFailedError(`${path} is a git repository already. Open it instead.`);
  }

  const branch = (await configuredDefaultBranch(path))
    ? []
    : [`--initial-branch=${DEFAULT_BRANCH}`];
  try {
    await runGit(homedir(), ["init", "--quiet", ...branch, "--", path]);
  } catch (error) {
    throw error instanceof GitError ? new InitFailedError(failure(error.stderr)) : error;
  }
  return path;
}

/**
 * The branch the user's `init.defaultBranch` names, if they've set one, for a repository about to
 * be created at `path`: from their global and system config, as `git init` reads it, and not a
 * repository's that the home folder may be in.
 */
async function configuredDefaultBranch(path: string): Promise<string | undefined> {
  const name = await runGit(homedir(), ["config", "--get", "init.defaultBranch"], {
    // Not there yet, so there's no repository config to read.
    env: { GIT_DIR: join(path, ".git") },
  }).catch(() => ""); // Exits with 1 when it's not set.
  return name.trim() || undefined;
}

/**
 * Why git couldn't create the repository, from what it wrote to stderr: its reasons without the
 * `fatal:` before them, e.g. `Cannot mkdir /srv/gitto: Permission denied`.
 */
export function failure(stderr: string): string {
  const lines = stderr
    .split(/\r?\n/)
    .map((line) => line.trim())
    // Its hints, like the one about naming the default branch, aren't why.
    .filter((line) => line && !line.startsWith("hint:"))
    .map((line) => line.replace(/^(fatal|error): /, ""))
    .map((line) => line.charAt(0).toUpperCase() + line.slice(1));
  return lines.join("\n") || "Git couldn't create the repository.";
}
