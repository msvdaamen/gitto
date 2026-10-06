import { implement } from "@orpc/server";

import { withRepo, type GitContext } from "../../core/middleware";
import { abortOperation, continueOperation, getOperation } from "./commands";
import { OperationContract } from "./contract";

const os = implement(OperationContract).$context<GitContext>();

export const operationRouter = os.router({
  get: os.get.use(withRepo).handler(({ context }) => getOperation(context.repo)),
  continue: os.continue
    .use(withRepo)
    .handler(({ context, input }) => continueOperation(context.repo, input.kind)),
  abort: os.abort
    .use(withRepo)
    .handler(({ context, input }) => abortOperation(context.repo, input.kind)),
});
