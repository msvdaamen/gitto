import { oc } from "@orpc/contract";

import { RepositoryInput } from "../../input";
import { OverviewSchema } from "./schema";

export const OverviewContract = {
  /**
   * What the home page shows of a repository, beyond its status: where it's hosted, when it was
   * last fetched and committed to, and what was last done in it.
   */
  get: oc.input(RepositoryInput).output(OverviewSchema),
};
