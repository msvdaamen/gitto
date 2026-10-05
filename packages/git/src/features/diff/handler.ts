import { implement } from "@orpc/server";

import { withRepo, type GitContext } from "../../core/middleware";
import {
  getBlob,
  getCommitFilePatch,
  getCommitFiles,
  getStagedFilePatch,
  getUnstagedFilePatch,
} from "./commands";
import { DiffContract } from "./contract";
import { readWorkingTreeFile } from "./working-tree";

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
  unstagedFilePatch: os.unstagedFilePatch
    .use(withRepo)
    .handler(({ context, input, signal }) => getUnstagedFilePatch(context.repo, input, signal)),
  stagedFilePatch: os.stagedFilePatch
    .use(withRepo)
    .handler(({ context, input, signal }) => getStagedFilePatch(context.repo, input, signal)),
  blob: os.blob
    .use(withRepo)
    .handler(({ context, input, signal }) => getBlob(context.repo, input.oid, signal)),
  workingTreeFile: os.workingTreeFile
    .use(withRepo)
    .handler(({ context, input }) => readWorkingTreeFile(context.repo, input.path)),
});
