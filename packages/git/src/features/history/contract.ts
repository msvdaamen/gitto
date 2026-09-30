import { oc } from "@orpc/contract";
import * as z from "zod";

import { RepositoryInput } from "../../input";
import { CommitSchema } from "./schema";

export const HistoryContract = {
  /** History of all branches, remotes and tags, newest first in topological order. */
  log: oc
    .input(
      RepositoryInput.extend({
        limit: z.number().int().positive().max(1000).default(200),
        skip: z.number().int().nonnegative().default(0),
      }),
    )
    .output(z.array(CommitSchema)),
};
