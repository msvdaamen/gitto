import { oc } from "@orpc/contract";
import * as z from "zod";

import { RepositoryInput, Sha } from "../../input";
import { ChangedFileSchema, WorkingTreeFilesSchema } from "./schema";

export const DiffContract = {
  /** Files changed by a commit, compared to its first parent. */
  commitFiles: oc.input(RepositoryInput.extend({ sha: Sha })).output(z.array(ChangedFileSchema)),
  /** Staged and unstaged changes, compared to HEAD and the index respectively. */
  workingTreeFiles: oc.input(RepositoryInput).output(WorkingTreeFilesSchema),
};
