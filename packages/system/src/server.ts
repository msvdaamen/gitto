import { implement } from "@orpc/server";

import { SystemContract } from "./contract";
import type { UpdateState } from "./schema";

/** Native OS capabilities, provided by the Electron main process. */
export interface SystemContext {
  selectFolder(): Promise<string | null>;
  updates: Updates;
}

/** Gitto's own updates. */
export interface Updates {
  /** The state now, then whenever it changes, until `signal` aborts. */
  watch(signal?: AbortSignal): AsyncGenerator<UpdateState>;
  /** Quits, installs the downloaded update and starts it. Does nothing until one is ready. */
  install(): void;
}

const os = implement(SystemContract).$context<SystemContext>();

export const systemRouter = os.router({
  selectFolder: os.selectFolder.handler(({ context }) => context.selectFolder()),
  update: {
    watch: os.update.watch.handler(({ context, signal }) => context.updates.watch(signal)),
    install: os.update.install.handler(({ context }) => context.updates.install()),
  },
});
