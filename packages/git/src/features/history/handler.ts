import { implement } from "@orpc/server";

import { withRepo, type GitContext } from "../../core/middleware";
import { getCommit, getVersionedLog } from "./commands";
import { HistoryContract } from "./contract";

const os = implement(HistoryContract).$context<GitContext>();

export const historyRouter = os.router({
  log: os.log
    .use(withRepo)
    .handler(({ context, input, signal }) =>
      getVersionedLog(context.repo, input, input.since, signal),
    ),
  commit: os.commit
    .use(withRepo)
    .handler(({ context, input, signal }) => getCommit(context.repo, input.sha, signal)),
});
