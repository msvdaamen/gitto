import { implement } from "@orpc/server";

import { withRepo, type GitContext } from "../../core/middleware";
import { getBlob, getCommitFilePatch, getCommitFiles } from "./commands";
import { DiffContract } from "./contract";

const os = implement(DiffContract).$context<GitContext>();

export const diffRouter = os.router({
  commitFiles: os.commitFiles
    .use(withRepo)
    .handler(({ context, input, signal }) => getCommitFiles(context.repo, input.sha, signal)),
  commitFilePatch: os.commitFilePatch
    .use(withRepo)
    .handler(({ context, input, signal }) =>
      getCommitFilePatch(context.repo, input.sha, input, signal),
    ),
  blob: os.blob
    .use(withRepo)
    .handler(({ context, input, signal }) => getBlob(context.repo, input.oid, signal)),
});
