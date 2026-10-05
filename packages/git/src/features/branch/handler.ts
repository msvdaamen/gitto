import { implement } from "@orpc/server";

import { withRepo, type GitContext } from "../../core/middleware";
import { createBranch, switchBranch } from "./commands";
import { BranchContract } from "./contract";

const os = implement(BranchContract).$context<GitContext>();

export const branchRouter = os.router({
  create: os.create
    .use(withRepo)
    .handler(({ context, input }) => createBranch(context.repo, input.name, input.from)),
  switch: os.switch
    .use(withRepo)
    .handler(({ context, input }) => switchBranch(context.repo, input.ref)),
});
