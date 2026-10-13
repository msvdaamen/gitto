import { oc } from "@orpc/contract";
import * as z from "zod";

import { RepositoryInput } from "../../input";
import { PushTargetSchema } from "./schema";

export const RemoteContract = {
  /** The remotes, by name, as `git remote` lists them. */
  list: oc.input(RepositoryInput).output(z.array(z.string())),
  /** Fetches every remote, pruning branches deleted there. */
  fetch: oc.input(RepositoryInput),
  /**
   * Pulls a branch's upstream into it: the current branch's, or the local branch `branch`'s, by
   * its short name. One that isn't checked out is only fast-forwarded; that fails with
   * PRECONDITION_FAILED, doing nothing, if it diverged from its upstream, or is in use in a
   * worktree.
   */
  pull: oc.input(RepositoryInput.extend({ branch: z.string().min(1).optional() })),
  /**
   * Pushes the current branch, setting its upstream if it has none. With `force`, replaces the
   * remote branch's commits, unless it has ones the branch never had. Fails with the code
   * `PUSH_REWRITTEN` when the remote branch has commits the branch had before it was rewritten,
   * which `force` replaces, and `PUSH_BEHIND` when it has others, to pull.
   */
  push: oc.input(RepositoryInput.extend({ force: z.boolean() })),
  /** Where `push` pushes the current branch, and how many commits it sends; or why it can't. */
  pushTarget: oc.input(RepositoryInput).output(PushTargetSchema),
};
