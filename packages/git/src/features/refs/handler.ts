import { implement } from "@orpc/server";

import { withRepo, type GitContext } from "../../core/middleware";
import { listRefs } from "./commands";
import { RefsContract } from "./contract";

const os = implement(RefsContract).$context<GitContext>();

export const refsRouter = os.router({
  list: os.list.use(withRepo).handler(async ({ context, input, signal }) => {
    const refs = await listRefs(context.repo, signal);
    return refs.version === input.since ? { unchanged: true as const } : refs;
  }),
});
