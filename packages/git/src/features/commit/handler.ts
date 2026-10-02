import { implement } from "@orpc/server";

import { withRepo, type GitContext } from "../../core/middleware";
import { createCommit, getCommitMessage } from "./commands";
import { CommitContract } from "./contract";

const os = implement(CommitContract).$context<GitContext>();

export const commitRouter = os.router({
  create: os.create
    .use(withRepo)
    .handler(({ context, input }) =>
      createCommit(context.repo, input.message, { amend: input.amend }),
    ),
  message: os.message
    .use(withRepo)
    .handler(({ context, input, signal }) => getCommitMessage(context.repo, input.sha, signal)),
});
