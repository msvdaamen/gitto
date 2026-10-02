import { oc } from "@orpc/contract";
import * as z from "zod";

import { RepositoryInput, Sha } from "../../input";
import { CommitSchema, LogPageSchema } from "./schema";

export const HistoryContract = {
  /**
   * History of all branches, remotes and tags, most recently committed first across all of them,
   * like GitKraken; a commit still always comes after its children. Sorting by commit date keeps
   * rebased and amended work at the top, where sorting by author date would bury it.
   *
   * `limit` commits, after the first `skip`: fewer means that's where the history ends. With the
   * `version` of the same page the caller already has, it returns `{ unchanged: true }` if nothing
   * changed, so the commits aren't sent (and laid out by the renderer) again for nothing.
   */
  log: oc
    .input(
      RepositoryInput.extend({
        limit: z.number().int().positive().max(50_000).default(200),
        skip: z.number().int().nonnegative().default(0),
        since: z.string().optional(),
      }),
    )
    .output(z.union([z.object({ unchanged: z.literal(true) }), LogPageSchema])),
  /** A single commit, e.g. the one selected in the history. */
  commit: oc.input(RepositoryInput.extend({ sha: Sha })).output(CommitSchema),
};
