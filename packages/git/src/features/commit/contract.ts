import { oc } from "@orpc/contract";
import * as z from "zod";

import { RepositoryInput, Sha } from "../../input";

export const CommitContract = {
  /**
   * Commits what's staged. With `amend`, HEAD's full SHA, replaces that commit instead, with what's
   * staged added to it and the new message; nothing needs to be staged to only reword it. Fails if
   * HEAD has moved on since, so another commit is never amended by mistake.
   */
  create: oc.input(
    RepositoryInput.extend({
      message: z.string().trim().min(1),
      amend: z
        .string()
        .regex(/^[0-9a-f]{40}([0-9a-f]{24})?$/)
        .optional(),
    }),
  ),
  /** A commit's full message, e.g. the last one's to amend it. */
  message: oc.input(RepositoryInput.extend({ sha: Sha })).output(z.string()),
  /**
   * The remote branch `git push` would update that already has the commit, e.g. `origin/feature`;
   * `null` if amending it needs no force-push.
   */
  pushedTo: oc.input(RepositoryInput.extend({ sha: Sha })).output(z.string().nullable()),
};
