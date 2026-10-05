import { realpath } from "node:fs/promises";

import type { Repo } from "./repo";

export interface GitDirs {
  /** This worktree's git directory: HEAD, the index. */
  gitDir: string;
  /** The git directory shared by all worktrees: refs, config. The same as `gitDir`, usually. */
  commonDir: string;
  excludeFile: string;
}

/** The repository's git directories, symlinks resolved (that's how the watcher reports paths). */
export async function gitDirs(repo: Repo): Promise<GitDirs> {
  const output = await repo.read([
    "rev-parse",
    "--path-format=absolute",
    "--git-dir",
    "--git-common-dir",
    "--git-path",
    "info/exclude",
  ]);
  const [gitDir = "", commonDir = "", excludeFile = ""] = output.split("\n");
  const [realGitDir, realCommonDir] = await Promise.all([realpath(gitDir), realpath(commonDir)]);
  return { gitDir: realGitDir, commonDir: realCommonDir, excludeFile };
}
