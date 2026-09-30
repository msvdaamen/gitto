import { oc } from "@orpc/contract";
import * as z from "zod";

import { RepositorySchema } from "./types";

export const RepositoryContract = {
  list: oc.output(z.array(RepositorySchema)),
  /** Adds the git repository at `path`; returns the existing entry if it was already added. */
  add: oc.input(z.object({ path: z.string() })).output(RepositorySchema),
  /** Removes the repository from Gitto; the folder on disk is left untouched. */
  remove: oc.input(z.object({ id: z.uuidv7() })),
};
