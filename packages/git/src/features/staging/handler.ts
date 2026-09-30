import { implement } from "@orpc/server";

import { withRepo, type GitContext } from "../../core/middleware";
import { stage, unstage } from "./commands";
import { StagingContract } from "./contract";

const os = implement(StagingContract).$context<GitContext>();

export const stagingRouter = os.router({
  stage: os.stage.use(withRepo).handler(({ context, input }) => stage(context.repo, input.paths)),
  unstage: os.unstage
    .use(withRepo)
    .handler(({ context, input }) => unstage(context.repo, input.paths)),
});
