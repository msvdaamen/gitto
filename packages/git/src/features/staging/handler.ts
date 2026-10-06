import { implement } from "@orpc/server";

import { withRepo, type GitContext } from "../../core/middleware";
import {
  discard,
  discardAll,
  stage,
  stageAll,
  stageLines,
  unstage,
  unstageAll,
  unstageLines,
} from "./commands";
import { StagingContract } from "./contract";

const os = implement(StagingContract).$context<GitContext>();

export const stagingRouter = os.router({
  stage: os.stage.use(withRepo).handler(({ context, input }) => stage(context.repo, input.paths)),
  unstage: os.unstage
    .use(withRepo)
    .handler(({ context, input }) => unstage(context.repo, input.paths)),
  stageAll: os.stageAll.use(withRepo).handler(({ context }) => stageAll(context.repo)),
  unstageAll: os.unstageAll.use(withRepo).handler(({ context }) => unstageAll(context.repo)),
  stageLines: os.stageLines.use(withRepo).handler(async ({ context, input }) => ({
    patch: await stageLines(context.repo, input, input.patch, input.lines),
  })),
  unstageLines: os.unstageLines.use(withRepo).handler(async ({ context, input }) => ({
    patch: await unstageLines(context.repo, input, input.patch, input.lines),
  })),
  discard: os.discard
    .use(withRepo)
    .handler(({ context, input }) => discard(context.repo, input, input.side)),
  discardAll: os.discardAll.use(withRepo).handler(({ context }) => discardAll(context.repo)),
});
