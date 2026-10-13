import { oc } from "@orpc/contract";
import * as z from "zod";

import { RepositoryInput, Sha } from "../../input";
import { CommitSchema } from "./schema";

export const HistoryContract = {
  /**
   * History of all branches, remotes and tags, most recently committed first across all of them,
   * like GitKraken; a commit still always comes after its children. Sorting by commit date keeps
   * rebased and amended work at the top, where sorting by author date would bury it.
   */
  log: oc
    .input(
      RepositoryInput.extend({
        // Enough for the history to reload all it has loaded at once, up to a point: git walks the
        // whole history to sort it anyway, so it's little slower than a page of a few hundred.
        limit: z.number().int().positive().max(10_000).default(200),
        skip: z.number().int().nonnegative().default(0),
      }),
    )
    .output(z.array(CommitSchema)),
  /** A single commit, e.g. the one selected in the history. */
  commit: oc.input(RepositoryInput.extend({ sha: Sha })).output(CommitSchema),
};
