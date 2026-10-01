import { eventIterator, oc } from "@orpc/contract";
import * as z from "zod";

import { RepositoryInput } from "../../input";
import { GitDirChangeSchema } from "./schema";

export const WatchContract = {
  /**
   * Emits what changed in the git directory, e.g. a commit, a checkout or staging from another
   * tool. Watches a handful of folders, so it's cheap to keep open.
   */
  gitDir: oc.input(RepositoryInput).output(eventIterator(z.array(GitDirChangeSchema))),
  /**
   * Emits whenever a file in the working tree changes. This watches the whole tree (except what
   * git ignores), so only keep it open while someone is looking.
   */
  workingTree: oc.input(RepositoryInput).output(eventIterator(z.null())),
};
