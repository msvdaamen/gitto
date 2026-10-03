import { oc } from "@orpc/contract";
import * as z from "zod";

import { RepositoryInput } from "../../input";
import { RefListSchema } from "./schema";

export const RefsContract = {
  /**
   * Local branches, remote branches and tags. With the `version` of the refs the caller already
   * has, it returns `{ unchanged: true }` if none changed. A big repository has thousands, which
   * take a while to send and for the renderer to process, and a refetch often finds them as they
   * were: after a fetch that brought nothing new, say, or a config change.
   */
  list: oc
    .input(RepositoryInput.extend({ since: z.string().optional() }))
    .output(z.union([z.object({ unchanged: z.literal(true) }), RefListSchema])),
};
