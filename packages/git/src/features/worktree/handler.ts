import { implement } from "@orpc/server";

import { withRepo, type GitContext } from "../../core/middleware";
import { addWorktree, listWorktrees, removeWorktree } from "./commands";
import { WorktreeContract } from "./contract";

const os = implement(WorktreeContract).$context<GitContext>();

export const worktreeRouter = os.router({
  list: os.list.use(withRepo).handler(({ context, signal }) => listWorktrees(context.repo, signal)),
  add: os.add
    .use(withRepo)
    .handler(({ context, input }) =>
      addWorktree(context.repo, input.path, input.branch, input.newBranch),
    ),
  remove: os.remove
    .use(withRepo)
    .handler(({ context, input }) => removeWorktree(context.repo, input.path)),
});
