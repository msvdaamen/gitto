import { oc } from "@orpc/contract";

import { RepositoryInput } from "../../input";

export const RemoteContract = {
  /** Pulls the current branch's upstream into it. */
  pull: oc.input(RepositoryInput),
};
