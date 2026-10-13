import { oc } from "@orpc/contract";
import * as z from "zod";

import { BranchRef, RepositoryInput } from "../../input";
import { MergeOutcomeSchema } from "./schema";

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
      from: BranchRef.optional(),
    }),
  ),
  /**
   * Switches to a branch, by its full ref name, taking the uncommitted changes along. For a remote
   * branch, that's the local branch tracking it, or a new one that does. When the changes conflict
   * with it, it's switched to anyway, and fails saying they're kept in the stash.
   */
  switch: oc.input(RepositoryInput.extend({ ref: BranchRef })),
  /**
   * Deletes a branch, by its full ref name, with the commits only it has. A local branch is deleted
   * here only. A remote branch is deleted on its remote (or only here, if the remote's gone), and so
   * are the local branches tracking it. That fails with PRECONDITION_FAILED, deleting nothing, if
   * one of them is checked out, or being rebased or bisected, or the remote's fetch settings don't
   * fetch the branch from one there.
   */
  delete: oc.input(RepositoryInput.extend({ ref: BranchRef })),
  /**
   * Merges a branch, by its full ref name, into the checked-out one, `into`, as the user saw it:
   * says whether it was up to date, fast-forwarded, merged, or stopped at conflicts, left to resolve.
   * Fails with PRECONDITION_FAILED, doing nothing, while HEAD is detached or an operation or
   * conflicts are under way; with CONFLICT if another branch is checked out by now, or the merge
   * stopped before committing, at a hook say.
   */
  merge: oc
    .input(RepositoryInput.extend({ ref: BranchRef, into: z.string().min(1) }))
    .output(MergeOutcomeSchema),
};
