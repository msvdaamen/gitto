import { implement } from "@orpc/server";

import { withRepo, type GitContext } from "../../core/middleware";
import { listStashes, popStash, pushStash } from "./commands";
import { StashContract } from "./contract";

const os = implement(StashContract).$context<GitContext>();

export const stashRouter = os.router({
  list: os.list.use(withRepo).handler(({ context, signal }) => listStashes(context.repo, signal)),
  push: os.push.use(withRepo).handler(({ context }) => pushStash(context.repo)),
  pop: os.pop.use(withRepo).handler(({ context, input }) => popStash(context.repo, input.sha)),
});
