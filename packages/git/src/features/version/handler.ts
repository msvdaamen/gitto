import { implement } from "@orpc/server";

import type { GitContext } from "../../core/middleware";
import { VersionContract } from "./contract";

const os = implement(VersionContract).$context<GitContext>();

export const versionRouter = os.router({
  check: os.check.handler(({ context }) => context.gitVersion.check()),
});
