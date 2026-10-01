import { eventIterator, oc } from "@orpc/contract";
import * as z from "zod";

import { RepositoryInput } from "../../input";
import { GitDirChangeSchema } from "./schema";

export const WatchContract = {
  /**
   * Emits what changed in the git directory, e.g. a commit, a checkout or staging from another
   * tool. Edits to files in the working tree don't show up here; poll the status for those.
   */
  gitDir: oc.input(RepositoryInput).output(eventIterator(z.array(GitDirChangeSchema))),
};
