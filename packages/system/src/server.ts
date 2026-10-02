import { implement } from "@orpc/server";

import { SystemContract } from "./contract";

/** Native OS capabilities, provided by the Electron main process. */
export interface SystemContext {
  selectFolder(): Promise<string | null>;
}

const os = implement(SystemContract).$context<SystemContext>();

export const systemRouter = os.router({
  selectFolder: os.selectFolder.handler(({ context }) => context.selectFolder()),
});
