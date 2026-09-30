import { eventIterator, oc } from "@orpc/contract";
import * as z from "zod";

import { RepositoryInput } from "../../input";

export const WatchContract = {
  /** Emits whenever the working tree or `.git` changes; refetch whatever you're showing. */
  changes: oc.input(RepositoryInput).output(eventIterator(z.null())),
};
