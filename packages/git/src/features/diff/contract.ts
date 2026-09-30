import { oc } from "@orpc/contract";
import * as z from "zod";

import { RepositoryInput } from "../../input";
import { ChangedFileSchema } from "./schema";

export const DiffContract = {
  /** Files changed by a commit, compared to its first parent. */
  commitFiles: oc
    .input(RepositoryInput.extend({ sha: z.string().regex(/^[0-9a-f]{4,64}$/) }))
    .output(z.array(ChangedFileSchema)),
  /** Staged and unstaged changes in the working tree, compared to HEAD. */
  workingTreeFiles: oc.input(RepositoryInput).output(z.array(ChangedFileSchema)),
};
