import { implement } from "@orpc/server";

import { toApiError, withGit, type GitContext } from "../../core/middleware";
import { cloneRepository } from "./commands";
import { CloneContract } from "./contract";

const os = implement(CloneContract).$context<GitContext>();

export const cloneRouter = os.router({
  run: os.run.use(withGit).handler(async function* ({ input, signal }) {
    // Its errors come as it's read, after `withGit` has returned it.
    try {
      return yield* cloneRepository(input.url, input.path, signal);
    } catch (error) {
      throw toApiError(error);
    }
  }),
});
