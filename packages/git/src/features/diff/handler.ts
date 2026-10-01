import { implement } from "@orpc/server";

import { withRepo, type GitContext } from "../../core/middleware";
import { getCommitFiles } from "./commands";
import { DiffContract } from "./contract";

const os = implement(DiffContract).$context<GitContext>();

export const diffRouter = os.router({
  commitFiles: os.commitFiles
    .use(withRepo)
    .handler(({ context, input, signal }) => getCommitFiles(context.repo, input.sha, signal)),
});
