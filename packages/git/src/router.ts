import { commitRouter } from "./features/commit/handler";
import { diffRouter } from "./features/diff/handler";
import { historyRouter } from "./features/history/handler";
import { refsRouter } from "./features/refs/handler";
import { stagingRouter } from "./features/staging/handler";
import { statusRouter } from "./features/status/handler";
import { watchRouter } from "./features/watch/handler";

/** Every git feature's router, matching `GitContract`. */
export const gitRouter = {
  status: statusRouter,
  history: historyRouter,
  diff: diffRouter,
  refs: refsRouter,
  staging: stagingRouter,
  commit: commitRouter,
  watch: watchRouter,
};
