import { eventIterator, oc } from "@orpc/contract";
import * as z from "zod";

import { CloneProgressSchema } from "./schema";

export const CloneContract = {
  /**
   * Clones the repository at `url` into `path`, a folder that's empty or not there yet, by its
   * full path (`~` for the home folder). Emits how far it's got as it goes, and returns the path it
   * cloned into. Fails with PRECONDITION_FAILED, leaving nothing behind, if the URL is wrong, git
   * couldn't sign in, or the folder is in use; stopping it partway leaves nothing either.
   */
  run: oc
    .input(
      z.object({
        url: z.string().regex(/\S/, "The URL is empty."),
        path: z.string().regex(/\S/, "The folder is empty."),
      }),
    )
    .output(eventIterator(CloneProgressSchema, z.string())),
};
