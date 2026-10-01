import { oc } from "@orpc/contract";

import { RepositoryInput } from "../../input";
import { WorkingTreeFilesSchema } from "../diff/schema";
import { StatusSchema } from "./schema";

export const StatusContract = {
  /**
   * The status, and the staged and unstaged changes with their line counts (compared to HEAD and
   * the index respectively). One call, so the working tree is only walked once.
   */
  get: oc.input(RepositoryInput).output(StatusSchema.extend({ changes: WorkingTreeFilesSchema })),
};
