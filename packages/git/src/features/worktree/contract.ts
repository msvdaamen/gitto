import { oc } from "@orpc/contract";
import * as z from "zod";

import { BranchRef, RepositoryInput } from "../../input";
import { WorktreeSchema } from "./schema";

export const WorktreeContract = {
  /** The worktrees, the main one first; the one this repository is, is marked `current`. */
  list: oc.input(RepositoryInput).output(z.array(WorktreeSchema)),
  /**
   * Checks a branch, by its full ref name, out in a new worktree at `path`, a folder that's empty
   * or not there yet; for a remote branch, the local branch tracking it, or a new one that does.
   * With `newBranch`, a branch of that name is created from it and checked out there instead.
   * Fails if the branch is checked out in a worktree already, or the folder is in use.
   */
  add: oc.input(
    RepositoryInput.extend({
      path: z.string().regex(/\S/, "The folder is empty."),
      branch: BranchRef,
      newBranch: z.string().regex(/\S/, "The name is empty.").optional(),
    }),
  ),
  /**
   * Removes a worktree, by its path as listed: its folder is deleted, uncommitted changes and all;
   * its branch is kept. Fails with PRECONDITION_FAILED for the main worktree, and for the one this
   * repository is; with CONFLICT if it's gone by now.
   */
  remove: oc.input(RepositoryInput.extend({ path: z.string() })),
};
