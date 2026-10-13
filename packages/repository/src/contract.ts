import { oc } from "@orpc/contract";
import * as z from "zod";

import { RepositorySchema } from "./types";

export const RepositoryContract = {
  /** In the order of their tabs. */
  list: oc.output(z.array(RepositorySchema)),
  /** Adds the git repository at `path`; returns the existing entry if it was already added. */
  add: oc.input(z.object({ path: z.string() })).output(RepositorySchema),
  /** Removes the repository from Gitto; the folder on disk is left untouched. */
  remove: oc.input(z.object({ id: z.uuidv7() })),
  /** Puts the tabs in the order of `ids`; repositories missing from it keep their place after. */
  reorder: oc.input(z.object({ ids: z.array(z.uuidv7()) })),
};
