import type { Repo } from "../../core/repo";
import { getWorkingTreeFiles } from "../diff/commands";
import type { WorkingTreeFiles } from "../diff/schema";
import { parseStatus, STATUS_ARGS } from "./parse";
import type { Status } from "./schema";

/**
 * A status slower than this is likely slowed down by files whose timestamps changed but whose
 * content didn't, like after a fresh clone, a branch switch or a formatter run. Reads don't refresh
 * the index (see GIT_OPTIONAL_LOCKS in the runner), so until something does, every status reads
 * those files again: seconds, in a big repository, on each change to the working tree.
 */
const SLOW_STATUS_MS = 200;

/** The status, and the changed files with their line counts: all the uncommitted changes show. */
export async function getStatus(
  repo: Repo,
  signal?: AbortSignal,
): Promise<Status & { changes: WorkingTreeFiles }> {
  const started = performance.now();
  const status = parseStatus(await repo.read(STATUS_ARGS, { signal }));
  // Refreshed afterwards, rather than taking the index lock for every status. If that updates the
  // index, the git directory watcher has the status read again, now without those files.
  if (performance.now() - started > SLOW_STATUS_MS) void repo.refreshIndex();
  return { ...status, changes: await getWorkingTreeFiles(repo, status.files, signal) };
}
