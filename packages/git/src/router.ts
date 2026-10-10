import { branchRouter } from "./features/branch/handler";
import { commitRouter } from "./features/commit/handler";
import { conflictsRouter } from "./features/conflicts/handler";
import { diffRouter } from "./features/diff/handler";
import { historyRouter } from "./features/history/handler";
import { operationRouter } from "./features/operation/handler";
import { overviewRouter } from "./features/overview/handler";
import { refsRouter } from "./features/refs/handler";
import { remoteRouter } from "./features/remote/handler";
import { stagingRouter } from "./features/staging/handler";
import { stashRouter } from "./features/stash/handler";
import { statusRouter } from "./features/status/handler";
import { userRouter } from "./features/user/handler";
import { versionRouter } from "./features/version/handler";
import { watchRouter } from "./features/watch/handler";
import { worktreeRouter } from "./features/worktree/handler";

/** Every git feature's router, matching `GitContract`. */
export const gitRouter = {
  status: statusRouter,
  history: historyRouter,
  diff: diffRouter,
  refs: refsRouter,
  branch: branchRouter,
  staging: stagingRouter,
  commit: commitRouter,
  remote: remoteRouter,
  stash: stashRouter,
  worktree: worktreeRouter,
  conflicts: conflictsRouter,
  operation: operationRouter,
  overview: overviewRouter,
  watch: watchRouter,
  version: versionRouter,
  user: userRouter,
};
