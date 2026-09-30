import { oc } from "@orpc/contract";
import * as z from "zod";

import { RepositoryInput } from "../../input";
import { RefSchema } from "./schema";

export const RefsContract = {
  /** Local branches, remote branches and tags. */
  list: oc.input(RepositoryInput).output(z.array(RefSchema)),
};
