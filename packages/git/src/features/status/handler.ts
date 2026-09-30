import { implement } from "@orpc/server";

import { withRepo, type GitContext } from "../../core/middleware";
import { getStatus } from "./commands";
import { StatusContract } from "./contract";

const os = implement(StatusContract).$context<GitContext>();

export const statusRouter = os.router({
  get: os.get.use(withRepo).handler(({ context, signal }) => getStatus(context.repo, signal)),
});
