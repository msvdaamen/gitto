import { oc } from "@orpc/contract";
import * as z from "zod";

import { RepositoryInput } from "../../input";

/** A local or remote branch, by its full ref name. */
const Branch = z.string().regex(/^refs\/(heads|remotes)\/./, "Not a branch.");

export const BranchContract = {
  /**
   * Creates a branch and switches to it, taking the uncommitted changes along: at HEAD, or at the
   * branch `from`, by its full ref name. Fails if the name is taken or isn't a valid branch name.
   * When the changes conflict with `from`, it's switched to anyway, and fails saying they're kept
   * in the stash.
   */
  create: oc.input(
    RepositoryInput.extend({
      name: z.string().regex(/\S/, "The name is empty."),
      from: Branch.optional(),
    }),
  ),
  /**
   * Switches to a branch, by its full ref name, taking the uncommitted changes along. For a remote
   * branch, that's the local branch tracking it, or a new one that does. When the changes conflict
   * with it, it's switched to anyway, and fails saying they're kept in the stash.
   */
  switch: oc.input(RepositoryInput.extend({ ref: Branch })),
};
