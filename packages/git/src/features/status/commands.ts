import { createHash } from "node:crypto";

import type { Repo } from "../../core/repo";
import type { FileStatus } from "../../schema";
import type { FileChange } from "../diff/schema";
import { parseStatus, STATUS_ARGS } from "./parse";
import type { StatusCounts, StatusFile, Uncommitted, WorkingTreeFiles } from "./schema";

/**
 * The status and the changed files: everything `git status` reports, in one run. It says what
 * changed on both sides of the index, so no diffs are needed on top; those only add line counts,
 * which take diffing every changed file (see `getLineCounts`).
 */
export async function getStatus(repo: Repo, signal?: AbortSignal): Promise<Uncommitted> {
  const output = await repo.read(STATUS_ARGS, { signal });
  const { files, ...status } = parseStatus(output);
  // Everything shown comes from git's output, so the same output means the same status.
  const version = createHash("sha1").update(output).digest("hex");
  return { ...status, counts: countFiles(files), changes: toChanges(files), version };
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
 * The files split into what's staged and what isn't, in git's order (by path), with the untracked
 * files after the other unstaged ones. A conflict is on both sides, like git's diffs have it.
 */
function toChanges(files: StatusFile[]): WorkingTreeFiles {
  const staged: FileChange[] = [];
  const unstaged: FileChange[] = [];
  const untracked: FileChange[] = [];
  for (const file of files) {
    if (file.staged) staged.push(change(file, file.staged));
    if (file.unstaged === "untracked") untracked.push(change(file, file.unstaged));
    else if (file.unstaged) unstaged.push(change(file, file.unstaged));
  }
  return { staged, unstaged: [...unstaged, ...untracked] };
}

/** `file`'s change on one side of the index; its previous path belongs to the side that moved it. */
function change(file: StatusFile, status: FileStatus): FileChange {
  const moved = status === "renamed" || status === "copied";
  return { path: file.path, status, origPath: moved ? file.origPath : null };
}
