import { createHash } from "node:crypto";

import type { Repo } from "../../core/repo";
import { parseRefs, REFS_ARGS } from "./parse";
import type { RefList } from "./schema";

/** The local branches, remote branches and tags, with a version of them. */
export async function listRefs(repo: Repo, signal?: AbortSignal): Promise<RefList> {
  const output = await repo.read(REFS_ARGS, { signal });
  // Everything listed comes from git's output, so the same output means the same refs.
  return { refs: parseRefs(output), version: createHash("sha1").update(output).digest("hex") };
}
