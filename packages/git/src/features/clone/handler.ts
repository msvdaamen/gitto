import { implement } from "@orpc/server";

import { withGit, type GitContext } from "../../core/middleware";
import { cloneRepository } from "./commands";
import { CloneContract } from "./contract";

const os = implement(CloneContract).$context<GitContext>();

export const cloneRouter = os.router({
  run: os.run
    .use(withGit)
    .handler(({ input, signal }) => cloneRepository(input.url, input.path, signal)),
});
