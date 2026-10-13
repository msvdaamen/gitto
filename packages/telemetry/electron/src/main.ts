import { homedir } from "node:os";

import * as Sentry from "@sentry/electron/main";

import { scrubber } from "./scrub";

export { captureException } from "@sentry/electron/main";

type Init = {
  /** Where reports go; a build without one (made without SENTRY_DSN) sends none. */
  dsn: string;
  version: string;
  isPackaged: boolean;
};

/**
 * Sends errors and crashes to Sentry: the main process's, and the renderer's, which it sends
 * through this one. Has to run before the app's ready. Returns whether it does: not without a DSN.
 */
export function initTelemetry({ dsn, version, isPackaged }: Init): boolean {
  if (!dsn) return false;
  const environment = environmentOf(version, isPackaged);
  const scrub = scrubber(homedir());
  Sentry.init({
    dsn,
    environment,
    // A native crash's minidump is a copy of the crashed process's memory, which can hold what's in
    // the user's files. Without it, a crashed renderer or GPU process is still reported.
    // LocalVariables(Async) would send local variables' values, which can be too.
    integrations: (defaults) =>
      defaults.filter(
        ({ name }) => name !== "SentryMinidump" && !name.startsWith("LocalVariables"),
      ),
    // No IP address, local variables' values, or HTTP headers, bodies and query strings.
    dataCollection: {
      userInfo: false,
      stackFrameVariables: false,
      cookies: false,
      httpHeaders: false,
      httpBodies: [],
      urlQueryParams: false,
    },
    beforeSend: scrub,
  });
  return true;
}

/** Stops sending anything, once the user's turned it off. */
export async function stopTelemetry() {
  await Sentry.close();
}

function environmentOf(version: string, isPackaged: boolean) {
  if (!isPackaged) return "development";
  return version.includes("-nightly") ? "nightly" : "production";
}
