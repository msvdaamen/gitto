import { oc } from "@orpc/contract";
import * as z from "zod";

import { FileInput, FullSha, RepositoryInput } from "../../input";
import { ChangedFileSchema } from "../diff/schema";
import { StashSchema } from "./schema";

export const StashContract = {
  /** The stashes, newest first. */
  list: oc.input(RepositoryInput).output(z.array(StashSchema)),
  /** Stashes every uncommitted change, untracked files included, which leaves no changes behind. */
  push: oc.input(RepositoryInput),
  /** The files a stash changed compared to its base, untracked ones included. */
  files: oc.input(RepositoryInput.extend({ sha: FullSha })).output(z.array(ChangedFileSchema)),
  /** The patch of one file a stash changed, compared to its base, as in its files. */
  filePatch: oc.input(FileInput.extend({ sha: FullSha })).output(z.string()),
  /**
   * Puts the newest stash's changes back, and drops it. Takes its full SHA, and fails if another
   * stash has become the newest since, so it's always the one the user saw that's popped. A pop
   * that conflicts is left to resolve, and the stash kept.
   */
  pop: oc.input(RepositoryInput.extend({ sha: FullSha })),
};
