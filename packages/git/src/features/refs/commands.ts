import type { Repo } from "../../core/repo";
import { parseRefs, REFS_ARGS } from "./parse";
import type { Ref } from "./schema";

export async function listRefs(repo: Repo, signal?: AbortSignal): Promise<Ref[]> {
  return parseRefs(await repo.read(REFS_ARGS, { signal }));
}
