import { createHash } from "node:crypto";

import type { Repo } from "../../core/repo";
import type { FileStatus } from "../../schema";
import { markerFree } from "../conflicts/commands";
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
  const start = performance.now();
  const output = await repo.read(STATUS_ARGS, { signal });
  repo.statusTook(performance.now() - start);
  const { files, ...status } = parseStatus(output);
  const [{ changes, outputs }, resolved] = await Promise.all([
    getWorkingTreeFiles(repo, files, signal),
    resolvedConflicts(repo, files),
  ]);
  // Everything shown comes from git's output, so the same output means the same status, but for
  // which conflicts have markers left, which comes from the files.
  const hash = createHash("sha1");
  for (const part of [output, ...outputs, ...resolved]) hash.update(part).update("\0");
  return {
    uncommitted: {
      ...status,
      counts: countFiles(files),
      changes: { ...changes, markerFree: resolved },
      version: hash.digest("hex"),
    },
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
 * Past this many conflicted files, none is read to tell whether it still has conflict markers: that
 * is done on every change in the working tree.
 */
const MAX_CHECKED_CONFLICTS = 200;

/**
 * The conflicted text files in `files` that have no conflict markers left (see `markerFree`):
 * those changed or added on both sides, as files rather than links or submodules.
 */
function resolvedConflicts(repo: Repo, files: StatusFile[]): Promise<string[]> {
  const text = files.filter(
    ({ conflict }) =>
      conflict &&
      (conflict.xy === "UU" || conflict.xy === "AA") &&
      REGULAR_FILE.test(conflict.ours) &&
      REGULAR_FILE.test(conflict.theirs),
  );
  if (text.length === 0 || text.length > MAX_CHECKED_CONFLICTS) return Promise.resolve([]);
  return markerFree(
    repo,
    text.map((file) => file.path),
  );
}

/** A file's mode, executable or not, rather than a link's or a submodule's. */
const REGULAR_FILE = /^100(644|755)$/;

/**
 * Past this many characters of paths, the unstaged diff isn't limited to the changed files: the
 * command line would get too long (Windows allows 32k characters, git's own arguments included).
 */
const MAX_PATHSPEC_LENGTH = 16_000;

/**
 * Past this many changed files, their lines aren't counted. Counting means diffing every one of
 * them again whenever any file is saved: 0.2s for 1,000 modified files in vscode, 0.5s for 5,000
 * and 2.2s for 20,000, on top of a status that takes 0.1-0.3s.
 */
export const MAX_COUNTED_FILES = 1000;

/**
 * Staged and unstaged changes, with their line counts, for the files `git status` reported. The
 * status already walked the working tree, so this doesn't again: the untracked files come from
 * it, and the unstaged diff only looks at the files it says changed. With more files than
 * `MAX_COUNTED_FILES`, all of it comes from the status, which then has all there is to show.
 */
async function getWorkingTreeFiles(
  repo: Repo,
  files: StatusFile[],
  signal?: AbortSignal,
): Promise<{ changes: Omit<WorkingTreeFiles, "markerFree">; outputs: string[] }> {
  const untracked = files
    .filter((file) => file.unstaged === "untracked")
    .map((file) => uncounted(file, "untracked"));
  if (files.length - untracked.length > MAX_COUNTED_FILES) {
    const staged: ChangedFile[] = [];
    const unstaged: ChangedFile[] = [];
    for (const file of files) {
      if (file.staged) staged.push(uncounted(file, file.staged));
      if (file.unstaged && file.unstaged !== "untracked") {
        unstaged.push(uncounted(file, file.unstaged));
      }
    }
    return {
      changes: { staged, unstaged: [...unstaged, ...untracked], uncounted: true },
      outputs: [],
    };
  }

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
    unstaged: [...parseDiff(unstaged), ...untracked],
    uncounted: false,
  };
  return { changes, outputs: [staged, unstaged] };
}

/** `file`'s change on one side of the index, as the status has it: without line counts. */
function uncounted(file: StatusFile, status: FileStatus): ChangedFile {
  // Its previous path belongs to the side that moved it.
  const moved = status === "renamed" || status === "copied";
  return {
    path: file.path,
    status,
    origPath: moved ? file.origPath : null,
    additions: null,
    deletions: null,
    ...(file.submodule && { submodule: true as const }),
  };
}
