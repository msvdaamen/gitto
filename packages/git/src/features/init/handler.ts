import { implement } from "@orpc/server";

import { withGit, type GitContext } from "../../core/middleware";
import { initRepository } from "./commands";
import { InitContract } from "./contract";

const os = implement(InitContract).$context<GitContext>();

export const initRouter = os.router({
  run: os.run.use(withGit).handler(({ input }) => initRepository(input.path)),
});
