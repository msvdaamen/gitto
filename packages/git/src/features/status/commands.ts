import type { Repo } from "../../core/repo";
import { parseStatus, STATUS_ARGS } from "./parse";
import type { Status } from "./schema";

export async function getStatus(repo: Repo, signal?: AbortSignal): Promise<Status> {
  return parseStatus(await repo.read(STATUS_ARGS, { signal }));
}
