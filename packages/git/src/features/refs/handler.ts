import { implement } from "@orpc/server";

import { withRepo, type GitContext } from "../../core/middleware";
import { listRefs } from "./commands";
import { RefsContract } from "./contract";

const os = implement(RefsContract).$context<GitContext>();

export const refsRouter = os.router({
  list: os.list.use(withRepo).handler(({ context, signal }) => listRefs(context.repo, signal)),
});
