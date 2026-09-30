import { implement } from "@orpc/server";

import { systemContract } from "./contract";

/** Native OS capabilities, provided by the Electron main process. */
export interface SystemContext {
  selectFolder(): Promise<string | null>;
}

const os = implement(systemContract).$context<SystemContext>();

export const systemRouter = os.router({
  hello: os.hello.handler(({ input }) => {
    return { message: `Hello, ${input.name}!` };
  }),
  selectFolder: os.selectFolder.handler(({ context }) => context.selectFolder()),
});
