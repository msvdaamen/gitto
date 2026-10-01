import { implement } from "@orpc/server";

import { withRepo, type GitContext } from "../../core/middleware";
import { watchGitDir } from "./commands";
import { WatchContract } from "./contract";

const os = implement(WatchContract).$context<GitContext>();

export const watchRouter = os.router({
  gitDir: os.gitDir
    .use(withRepo)
    .handler(({ context, signal }) => watchGitDir(context.repo, signal)),
});
