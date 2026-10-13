import { oc } from "@orpc/contract";
import * as z from "zod";

import { RepositoryInput } from "../../input";

export const RemoteContract = {
  /** The remotes, by name, as `git remote` lists them. */
  list: oc.input(RepositoryInput).output(z.array(z.string())),
  /** Fetches every remote, pruning branches deleted there. */
  fetch: oc.input(RepositoryInput),
  /** Pulls the current branch's upstream into it. */
  pull: oc.input(RepositoryInput),
};
