import type { Repo } from "../../core/repo";
import type { StatusFile } from "../status/schema";
import { parseDiff } from "./parse";
import type { ChangedFile, WorkingTreeFiles } from "./schema";

export async function getCommitFiles(
  repo: Repo,
  sha: string,
  signal?: AbortSignal,
): Promise<ChangedFile[]> {
  const output = await repo.read(
    [
      "diff-tree",
      "-r",
      "--root",
      "-M",
      "--raw",
      "--numstat",
      "-z",
      "--no-commit-id",
      "--diff-merges=first-parent",
      sha,
    ],
    { signal },
  );
  return parseDiff(output);
}

/**
 * Above this many characters of paths, the unstaged diff isn't limited to the changed files: the
 * command line would get too long (Windows allows 32k characters).
 */
const MAX_PATHSPEC_LENGTH = 16_000;

/**
 * Staged and unstaged changes, with their line counts, for the files `git status` reported. The
 * status already walked the working tree, so this doesn't again: the untracked files come from
 * it, and the unstaged diff only looks at the files it says changed.
 */
export async function getWorkingTreeFiles(
  repo: Repo,
  files: StatusFile[],
  signal?: AbortSignal,
): Promise<WorkingTreeFiles> {
  const hasStaged = files.some((file) => file.staged !== null);
  const unstagedPaths = files
    .filter((file) => file.unstaged !== null && file.unstaged !== "untracked")
    .map((file) => file.path);
  const pathspec =
    unstagedPaths.join("").length > MAX_PATHSPEC_LENGTH ? [] : ["--", ...unstagedPaths];

  const [staged, unstaged] = await Promise.all([
    // Without a HEAD commit, `--cached` diffs the index against the empty tree.
    hasStaged ? repo.read(["diff", "--cached", "--raw", "--numstat", "-z", "-M"], { signal }) : "",
    unstagedPaths.length > 0
      ? repo.read(["diff", "--raw", "--numstat", "-z", ...pathspec], { signal })
      : "",
  ]);
  return {
    staged: parseDiff(staged),
    unstaged: [
      ...parseDiff(unstaged),
      ...files
        .filter((file) => file.unstaged === "untracked")
        .map<ChangedFile>((file) => ({
          path: file.path,
          status: "untracked",
          origPath: null,
          additions: null,
          deletions: null,
        })),
    ],
  };
}
