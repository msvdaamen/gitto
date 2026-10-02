import { oc } from "@orpc/contract";
import * as z from "zod";

import { RepositoryInput } from "../../input";
import { UncommittedSchema } from "./schema";

export const StatusContract = {
  /**
   * The status, and the staged and unstaged changes with their line counts (compared to HEAD and
   * the index respectively). One call, so the working tree is only walked once. With the `version`
   * of the status the caller already has, it returns `{ unchanged: true }` if nothing changed, so a
   * big list of changes isn't sent (and processed by the renderer) again for nothing.
   */
  get: oc
    .input(RepositoryInput.extend({ since: z.string().optional() }))
    .output(z.union([z.object({ unchanged: z.literal(true) }), UncommittedSchema])),
};
