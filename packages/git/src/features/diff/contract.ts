import { oc } from "@orpc/contract";
import * as z from "zod";

import { RepositoryInput, Sha } from "../../input";
import { ChangedFileSchema, LineCountsSchema } from "./schema";

export const DiffContract = {
  /** Files changed by a commit, compared to its first parent. */
  commitFiles: oc.input(RepositoryInput.extend({ sha: Sha })).output(z.array(ChangedFileSchema)),
  /**
   * Line counts of the uncommitted changes to `files`, staged or not, by path. The status leaves
   * them out, as counting lines means diffing every file: ask for the files on show. A renamed
   * file's previous path is needed to count it as a rename. Files without changes on that side
   * (e.g. untracked ones) aren't in the answer.
   */
  lineCounts: oc
    .input(
      RepositoryInput.extend({
        staged: z.boolean(),
        files: z
          .array(z.object({ path: z.string().min(1), origPath: z.string().min(1).nullable() }))
          .min(1)
          .max(1000),
      }),
    )
    .output(z.array(LineCountsSchema)),
};
