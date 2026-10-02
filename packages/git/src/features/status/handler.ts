import { implement } from "@orpc/server";

import { withRepo, type GitContext } from "../../core/middleware";
import { getStatus } from "./commands";
import { StatusContract } from "./contract";
import { refreshStaleIndex } from "./refresh";

const os = implement(StatusContract).$context<GitContext>();

export const statusRouter = os.router({
  get: os.get.use(withRepo).handler(async ({ context, input, signal }) => {
    const start = performance.now();
    const status = await getStatus(context.repo, signal);
    // Not waited for: it only makes the statuses after this one faster.
    void refreshStaleIndex(context.repo, performance.now() - start);
    return status.version === input.since ? { unchanged: true as const } : status;
  }),
});
