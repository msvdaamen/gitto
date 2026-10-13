import { implement } from "@orpc/server";

import { SystemContract } from "./contract";
import type { TelemetryState, UpdateState, VersionChanges } from "./schema";

/** Native OS capabilities, provided by the Electron main process. */
export interface SystemContext {
  selectFolder(): Promise<string | null>;
  updates: Updates;
  changelog: WhatsNew;
  telemetry: Telemetry;
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

/** The user's setting for sending error reports. */
export interface Telemetry {
  state(): TelemetryState;
  /** Saves it; turning it off stops sending at once. */
  setEnabled(enabled: boolean): Promise<TelemetryState>;
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
  telemetry: {
    get: os.telemetry.get.handler(({ context }) => context.telemetry.state()),
    set: os.telemetry.set.handler(({ context, input }) =>
      context.telemetry.setEnabled(input.enabled),
    ),
  },
});
