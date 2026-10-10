import { implement } from "@orpc/server";

import { SystemContract } from "./contract";
import type { UpdateState, VersionChanges } from "./schema";

/** Native OS capabilities, provided by the Electron main process. */
export interface SystemContext {
  selectFolder(): Promise<string | null>;
  updates: Updates;
  changelog: WhatsNew;
}

/** Gitto's own updates. */
export interface Updates {
  /** The state now, then whenever it changes, until `signal` aborts. */
  watch(signal?: AbortSignal): AsyncGenerator<UpdateState>;
  /** Quits, installs the downloaded update and starts it. Does nothing until one is ready. */
  install(): void;
}

/** What changed in Gitto, from the changelog it's built with. */
export interface WhatsNew {
  /** The versions since the last one the user saw what changed in, up to this one. */
  unseen(): Promise<VersionChanges[]>;
  /** Every version the changelog has, up to this one. */
  all(): VersionChanges[];
  /** Remembers this version as seen. */
  seen(): Promise<void>;
}

const os = implement(SystemContract).$context<SystemContext>();

export const systemRouter = os.router({
  selectFolder: os.selectFolder.handler(({ context }) => context.selectFolder()),
  update: {
    watch: os.update.watch.handler(({ context, signal }) => context.updates.watch(signal)),
    install: os.update.install.handler(({ context }) => context.updates.install()),
  },
  changelog: {
    unseen: os.changelog.unseen.handler(({ context }) => context.changelog.unseen()),
    all: os.changelog.all.handler(({ context }) => context.changelog.all()),
    seen: os.changelog.seen.handler(({ context }) => context.changelog.seen()),
  },
});
