import { implement } from "@orpc/server";

import { withRepo, type GitContext } from "../../core/middleware";
import { createCommit, getCommitMessage, getPushedTo } from "./commands";
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
  pushedTo: os.pushedTo
    .use(withRepo)
    .handler(({ context, input, signal }) => getPushedTo(context.repo, input.sha, signal)),
});
