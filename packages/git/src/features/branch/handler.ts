import { implement } from "@orpc/server";

import { withRepo, type GitContext } from "../../core/middleware";
import { createBranch } from "./commands";
import { BranchContract } from "./contract";

const os = implement(BranchContract).$context<GitContext>();

export const branchRouter = os.router({
  create: os.create
    .use(withRepo)
    .handler(({ context, input }) => createBranch(context.repo, input.name)),
});
