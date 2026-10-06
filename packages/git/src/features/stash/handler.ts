import { implement } from "@orpc/server";

import { withRepo, type GitContext } from "../../core/middleware";
import {
  dropStash,
  getStashFilePatch,
  getStashFiles,
  listStashes,
  popStash,
  pushStash,
} from "./commands";
import { StashContract } from "./contract";

const os = implement(StashContract).$context<GitContext>();

export const stashRouter = os.router({
  list: os.list.use(withRepo).handler(({ context, signal }) => listStashes(context.repo, signal)),
  push: os.push.use(withRepo).handler(({ context }) => pushStash(context.repo)),
  files: os.files
    .use(withRepo)
    .handler(({ context, input, signal }) => getStashFiles(context.repo, input.sha, signal)),
  filePatch: os.filePatch
    .use(withRepo)
    .handler(({ context, input, signal }) =>
      getStashFilePatch(context.repo, input.sha, input, signal),
    ),
  pop: os.pop.use(withRepo).handler(({ context, input }) => popStash(context.repo, input.sha)),
  drop: os.drop.use(withRepo).handler(({ context, input }) => dropStash(context.repo, input.sha)),
});
