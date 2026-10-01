import { implement } from "@orpc/server";

import { withRepo, type GitContext } from "../../core/middleware";
import { watchGitDir, watchWorkingTree } from "./commands";
import { WatchContract } from "./contract";

const os = implement(WatchContract).$context<GitContext>();

export const watchRouter = os.router({
  gitDir: os.gitDir
    .use(withRepo)
    .handler(({ context, signal }) => watchGitDir(context.repo, signal)),
  workingTree: os.workingTree
    .use(withRepo)
    .handler(({ context, signal }) => watchWorkingTree(context.repo, signal)),
});
