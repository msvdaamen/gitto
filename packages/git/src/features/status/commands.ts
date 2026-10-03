import { createHash } from "node:crypto";

import type { Repo } from "../../core/repo";
import { parseDiff } from "../diff/parse";
import type { ChangedFile } from "../diff/schema";
import { parseStatus, STATUS_ARGS } from "./parse";
import type { StatusCounts, StatusFile, Uncommitted, WorkingTreeFiles } from "./schema";

/** The status, and the changed files with their line counts: all the uncommitted changes show. */
export function getStatus(repo: Repo, signal?: AbortSignal): Promise<Uncommitted> {
  const status = readStatus(repo, signal);
  // It's the UI that asks. Only shown once all of it is read: a status that fails isn't.
  repo.showStatus(
    status.then(
      ({ snapshot }) => snapshot,
      () => undefined,
    ),
  );
  return status.then(({ uncommitted }) => uncommitted);
}

/**
 * What `git status` reports right now, hashed. The same as an earlier one when HEAD, the index and
 * which files differ from it are: all but the changed files' contents, which the working tree's
 * watcher follows.
 */
export async function statusSnapshot(repo: Repo): Promise<string> {
  return snapshotOf(await repo.read(STATUS_ARGS));
}

function snapshotOf(output: string): string {
  return createHash("sha1").update(output).digest("hex");
}

async function readStatus(
  repo: Repo,
  signal?: AbortSignal,
): Promise<{ uncommitted: Uncommitted; snapshot: string }> {
  const output = await repo.read(STATUS_ARGS, { signal });
  const { files, ...status } = parseStatus(output);
  const { changes, outputs } = await getWorkingTreeFiles(repo, files, signal);
  // Everything shown comes from git's output, so the same output means the same status.
  const hash = createHash("sha1");
  for (const part of [output, ...outputs]) hash.update(part).update("\0");
  return {
    uncommitted: { ...status, counts: countFiles(files), changes, version: hash.digest("hex") },
    snapshot: snapshotOf(output),
  };
}

function countFiles(files: StatusFile[]): StatusCounts {
  let staged = 0;
  let unstaged = 0;
  let conflicted = 0;
  for (const file of files) {
    if (file.staged === "conflicted") conflicted++;
    else {
      if (file.staged) staged++;
      if (file.unstaged) unstaged++;
    }
  }
  return { files: files.length, staged, unstaged, conflicted };
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
): Promise<{ changes: WorkingTreeFiles; outputs: string[] }> {
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
  const changes = {
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
  return { changes, outputs: [staged, unstaged] };
}
