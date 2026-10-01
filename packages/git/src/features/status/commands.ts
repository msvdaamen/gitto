import type { Repo } from "../../core/repo";
import { getWorkingTreeFiles } from "../diff/commands";
import type { WorkingTreeFiles } from "../diff/schema";
import { parseStatus, STATUS_ARGS } from "./parse";
import type { Status } from "./schema";

/** The status, and the changed files with their line counts: all the uncommitted changes show. */
export async function getStatus(
  repo: Repo,
  signal?: AbortSignal,
): Promise<Status & { changes: WorkingTreeFiles }> {
  const status = parseStatus(await repo.read(STATUS_ARGS, { signal }));
  return { ...status, changes: await getWorkingTreeFiles(repo, status.files, signal) };
}
