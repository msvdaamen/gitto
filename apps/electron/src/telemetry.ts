import { readFileSync } from "node:fs";
import { writeFile } from "node:fs/promises";

import type { Telemetry } from "@gitto/system/server";

/**
 * Whether the user lets Gitto send error reports and how long it takes to start, kept in `file`.
 * It's on until they turn it off. Read before the app's ready, when Sentry has to start.
 */
export function readTelemetrySetting(file: string): boolean {
  try {
    const settings = JSON.parse(readFileSync(file, "utf8")) as { telemetry?: unknown };
    return settings.telemetry !== false;
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== "ENOENT") {
      console.error("[telemetry] Couldn't read", file, error);
    }
    return true;
  }
}

/**
 * The user's setting, kept in `file`, and whether Gitto's sending: only a build that can, which
 * started with it on. Turning it off stops it at once, with `stop`; turning it on only starts it
 * when Gitto restarts, as Sentry has to start before the app's ready.
 */
export class TelemetrySetting implements Telemetry {
  constructor(
    private readonly file: string,
    private readonly available: boolean,
    private enabled: boolean,
    private sending: boolean,
    private readonly stop: () => Promise<void>,
  ) {}

  state() {
    return { available: this.available, enabled: this.enabled, sending: this.sending };
  }

  async setEnabled(enabled: boolean) {
    await writeFile(this.file, `${JSON.stringify({ telemetry: enabled }, null, 2)}\n`);
    this.enabled = enabled;
    if (!enabled && this.sending) {
      this.sending = false;
      await this.stop();
    }
    return this.state();
  }
}
