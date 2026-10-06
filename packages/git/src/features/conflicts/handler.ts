import { implement } from "@orpc/server";

import { withRepo, type GitContext } from "../../core/middleware";
import { getConflict, keepSide, markResolved } from "./commands";
import { ConflictsContract } from "./contract";

const os = implement(ConflictsContract).$context<GitContext>();

export const conflictsRouter = os.router({
  get: os.get.use(withRepo).handler(({ context, input }) => getConflict(context.repo, input.path)),
  keep: os.keep
    .use(withRepo)
    .handler(({ context, input }) => keepSide(context.repo, input.path, input.side, input)),
  markResolved: os.markResolved
    .use(withRepo)
    .handler(({ context, input }) => markResolved(context.repo, input.path, input.version)),
});
