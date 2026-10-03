import { oc } from "@orpc/contract";
import * as z from "zod";

import { RepositoryInput } from "../../input";

export const BranchContract = {
  /**
   * Creates a branch at HEAD and switches to it, taking every uncommitted change along. Fails if
   * the name is taken or isn't a valid branch name.
   */
  create: oc.input(RepositoryInput.extend({ name: z.string().regex(/\S/, "The name is empty.") })),
  /**
   * Switches to a branch, by its full ref name, taking the uncommitted changes along. For a remote
   * branch, that's the local branch tracking it, or a new one that does. When the changes conflict
   * with it, it's switched to anyway, and fails saying they're kept in the stash.
   */
  switch: oc.input(
    RepositoryInput.extend({
      ref: z.string().regex(/^refs\/(heads|remotes)\/./, "Not a branch."),
    }),
  ),
};
