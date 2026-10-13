import { implement } from "@orpc/server";

import { withRepo, type GitContext } from "../../core/middleware";
import { fetchAll, listRemotes, pull } from "./commands";
import { RemoteContract } from "./contract";
import { getPushTarget, push } from "./push";

const os = implement(RemoteContract).$context<GitContext>();

export const remoteRouter = os.router({
  list: os.list
    .use(withRepo)
    .handler(({ context, signal }) => listRemotes(context.repo.read, signal)),
  fetch: os.fetch.use(withRepo).handler(({ context }) => fetchAll(context.repo)),
  pull: os.pull.use(withRepo).handler(({ context }) => pull(context.repo)),
  push: os.push
    .use(withRepo)
    .handler(({ context, input }) => push(context.repo, { force: input.force })),
  pushTarget: os.pushTarget.use(withRepo).handler(({ context }) => getPushTarget(context.repo)),
});
