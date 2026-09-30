import { oc } from "@orpc/contract";
import * as z from "zod";

import { RepositoryInput } from "../../input";

export const CommitContract = {
  /** Commits what's staged. */
  create: oc.input(RepositoryInput.extend({ message: z.string().trim().min(1) })),
};
