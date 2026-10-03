import { oc } from "@orpc/contract";
import * as z from "zod";

import { RepositoryInput } from "../../input";

export const BranchContract = {
  /**
   * Creates a branch at HEAD and switches to it, taking every uncommitted change along. Fails if
   * the name is taken or isn't a valid branch name.
   */
  create: oc.input(RepositoryInput.extend({ name: z.string().regex(/\S/, "The name is empty.") })),
};
