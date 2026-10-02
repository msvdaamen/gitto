import { oc } from "@orpc/contract";
import * as z from "zod";

import { RepositoryInput, Sha } from "../../input";

export const CommitContract = {
  /**
   * Commits what's staged. With `amend`, replaces the last commit instead, with what's staged added
   * to it and the new message; nothing needs to be staged to only reword it.
   */
  create: oc.input(
    RepositoryInput.extend({
      message: z.string().trim().min(1),
      amend: z.boolean().default(false),
    }),
  ),
  /** A commit's full message, e.g. the last one's to amend it. */
  message: oc.input(RepositoryInput.extend({ sha: Sha })).output(z.string()),
};
