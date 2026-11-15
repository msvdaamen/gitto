import { oc } from "@orpc/contract";
import * as z from "zod";

export const InitContract = {
  /**
   * Creates an empty repository in `path`, by its full path (`~` for the home folder), and returns
   * that path. The folder is made if it isn't there, and what's in it is kept if it is. It starts
   * on the branch the user's `init.defaultBranch` names, or `main`. Fails with
   * PRECONDITION_FAILED if the folder is a repository already, or git couldn't create one there.
   */
  run: oc
    .input(z.object({ path: z.string().regex(/\S/, "The folder is empty.") }))
    .output(z.string()),
};
