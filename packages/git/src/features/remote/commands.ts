import type { Repo } from "../../core/repo";

/**
 * Fetches every remote, and drops remote-tracking branches whose branch was deleted on the remote,
 * so the sidebar and history show the remotes as they are.
 */
export async function fetchAll(repo: Repo): Promise<void> {
  await repo.fetch(["fetch", "--all", "--prune", "--no-progress"]);
}
