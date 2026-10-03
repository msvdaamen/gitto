import { implement } from "@orpc/server";

import { withRepo, type GitContext } from "../../core/middleware";
import { fetchAll, pull } from "./commands";
import { RemoteContract } from "./contract";

const os = implement(RemoteContract).$context<GitContext>();

export const remoteRouter = os.router({
  fetch: os.fetch.use(withRepo).handler(({ context }) => fetchAll(context.repo)),
  pull: os.pull.use(withRepo).handler(({ context }) => pull(context.repo)),
});
