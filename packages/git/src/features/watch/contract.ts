import { eventIterator, oc } from "@orpc/contract";
import * as z from "zod";

import { RepositoryInput } from "../../input";
import { GitDirChangeSchema, TreeEventSchema } from "./schema";

export const WatchContract = {
  /**
   * Emits what changed in the git directory, e.g. a commit, a checkout or staging from another
   * tool. Watches a handful of folders, so it's cheap to keep open.
   */
  gitDir: oc.input(RepositoryInput).output(eventIterator(z.array(GitDirChangeSchema))),
  /**
   * Emits `ready` once it's watching, then `changed` whenever a file in the working tree changes.
   * Changes from before it's ready aren't reported, so whatever was loaded before needs loading
   * again then. This watches the whole tree (except what git ignores), so only keep it open while
   * someone is looking.
   */
  workingTree: oc.input(RepositoryInput).output(eventIterator(TreeEventSchema)),
};
