import type { Repo } from "../../core/repo";
import { parseDiff } from "../diff/parse";
import type { ChangedFile } from "../diff/schema";
import { parseStatus, STATUS_ARGS } from "./parse";
import type { Status, StatusFile, WorkingTreeFiles } from "./schema";

/** The status, and the changed files with their line counts: all the uncommitted changes show. */
export async function getStatus(
  repo: Repo,
  signal?: AbortSignal,
): Promise<Status & { changes: WorkingTreeFiles }> {
  const status = parseStatus(await repo.read(STATUS_ARGS, { signal }));
  return { ...status, changes: await getWorkingTreeFiles(repo, status.files, signal) };
}

/**
 * Past this many characters of paths, the unstaged diff isn't limited to the changed files: the
 * command line would get too long (Windows allows 32k characters, git's own arguments included).
 */
const MAX_PATHSPEC_LENGTH = 16_000;

/**
 * Staged and unstaged changes, with their line counts, for the files `git status` reported. The
 * status already walked the working tree, so this doesn't again: the untracked files come from
 * it, and the unstaged diff only looks at the files it says changed.
 */
async function getWorkingTreeFiles(
  repo: Repo,
  files: StatusFile[],
  signal?: AbortSignal,
): Promise<WorkingTreeFiles> {
  const hasStaged = files.some((file) => file.staged !== null);
  const unstagedPaths = files
    .filter((file) => file.unstaged !== null && file.unstaged !== "untracked")
    .map((file) => file.path);
  // Each path also takes a space and, on Windows, quotes if it has spaces.
  const length = unstagedPaths.reduce((sum, path) => sum + path.length + 3, 0);
  const pathspec = length > MAX_PATHSPEC_LENGTH ? [] : ["--", ...unstagedPaths];

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
