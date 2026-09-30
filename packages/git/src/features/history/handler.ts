import { implement } from "@orpc/server";

import { withRepo, type GitContext } from "../../core/middleware";
import { getCommit, getLog } from "./commands";
import { HistoryContract } from "./contract";

const os = implement(HistoryContract).$context<GitContext>();

export const historyRouter = os.router({
  log: os.log
    .use(withRepo)
    .handler(({ context, input, signal }) => getLog(context.repo, input, signal)),
  commit: os.commit
    .use(withRepo)
    .handler(({ context, input, signal }) => getCommit(context.repo, input.sha, signal)),
});
