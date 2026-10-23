import { implement } from "@orpc/server";

import { withRepo, type GitContext } from "../../core/middleware";
import { createCommit, getCommitMessage, getPushedTo, rewordCommit } from "./commands";
import { CommitContract } from "./contract";

const os = implement(CommitContract).$context<GitContext>();

export const commitRouter = os.router({
  create: os.create
    .use(withRepo)
    .handler(({ context, input }) =>
      createCommit(context.repo, input.message, { amend: input.amend }),
    ),
  reword: os.reword
    .use(withRepo)
    .handler(({ context, input }) => rewordCommit(context.repo, input.sha, input.message)),
  message: os.message
    .use(withRepo)
    .handler(({ context, input, signal }) => getCommitMessage(context.repo, input.sha, signal)),
  pushedTo: os.pushedTo
    .use(withRepo)
    .handler(({ context, input, signal }) => getPushedTo(context.repo, input.sha, signal)),
});
