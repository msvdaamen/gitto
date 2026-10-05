import { implement } from "@orpc/server";

import type { GitContext } from "../../core/middleware";
import { getUserName } from "./commands";
import { UserContract } from "./contract";

const os = implement(UserContract).$context<GitContext>();

export const userRouter = os.router({
  name: os.name.handler(() => getUserName()),
});
