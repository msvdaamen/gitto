import { implement } from "@orpc/server";

import { withRepo, type GitContext } from "../../core/middleware";
import { pull } from "./commands";
import { RemoteContract } from "./contract";

const os = implement(RemoteContract).$context<GitContext>();

export const remoteRouter = os.router({
  pull: os.pull.use(withRepo).handler(({ context }) => pull(context.repo)),
});
