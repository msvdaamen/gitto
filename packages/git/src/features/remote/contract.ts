import { oc } from "@orpc/contract";

import { RepositoryInput } from "../../input";

export const RemoteContract = {
  /** Fetches every remote, pruning branches deleted there. */
  fetch: oc.input(RepositoryInput),
};
