import { oc } from "@orpc/contract";

import { RepositoryInput } from "../../input";
import { OperationKindSchema, OperationSchema } from "./schema";

const KindInput = RepositoryInput.extend({ kind: OperationKindSchema });

export const OperationContract = {
  /** The merge, rebase, cherry-pick, revert or `git am` under way; `null` if none is. */
  get: oc.input(RepositoryInput).output(OperationSchema.nullable()),
  /**
   * Continues the operation `kind` once its conflicts are resolved, with the message git prepared.
   * Fails with PRECONDITION_FAILED if it isn't the one under way any more.
   */
  continue: oc.input(KindInput),
  /** Aborts the operation `kind`, which undoes it; fails like `continue`. */
  abort: oc.input(KindInput),
};
