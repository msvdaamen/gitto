import { implement } from "@orpc/server";

import { withRepo, type GitContext } from "../../core/middleware";
import { getOverview } from "./commands";
import { OverviewContract } from "./contract";

const os = implement(OverviewContract).$context<GitContext>();

export const overviewRouter = os.router({
  get: os.get.use(withRepo).handler(({ context, signal }) => getOverview(context.repo, signal)),
});
