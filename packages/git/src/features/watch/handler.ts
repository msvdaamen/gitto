import { implement } from "@orpc/server";

import { withRepo, type GitContext } from "../../core/middleware";
import { watchChanges } from "./commands";
import { WatchContract } from "./contract";

const os = implement(WatchContract).$context<GitContext>();

export const watchRouter = os.router({
  changes: os.changes
    .use(withRepo)
    .handler(({ context, signal }) => watchChanges(context.repo, signal)),
});
