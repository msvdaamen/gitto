import { implement } from "@orpc/server";

import { withRepo, type GitContext } from "../../core/middleware";
import { getAvatar } from "./commands";
import { AvatarsContract } from "./contract";

const os = implement(AvatarsContract).$context<GitContext>();

export const avatarsRouter = os.router({
  find: os.find
    .use(withRepo)
    .handler(({ context, input, signal }) => getAvatar(context.repo, input, signal)),
});
