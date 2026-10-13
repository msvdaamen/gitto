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
 * Sends errors, crashes and how long Gitto takes to start to Sentry: the main process's, and the
 * renderer's, which it sends through this one. Has to run before the app's ready. Returns whether it
 * does: not without a DSN.
 */
export function initTelemetry({ dsn, version, isPackaged }: Init): boolean {
  if (!dsn) return false;
  const environment = environmentOf(version, isPackaged);
  const scrub = scrubber(homedir());
  Sentry.init({
    dsn,
    environment,
    // The only traces are startups, one per launch. Nightlies are few and see the newest code, so
    // every one is kept; of releases, enough to see how long starting takes.
    tracesSampleRate: environment === "production" ? 0.2 : 1,
    integrations: (defaults) => [
      // A native crash's minidump is a copy of the crashed process's memory, which can hold what's
      // in the user's files. Without it, a crashed renderer or GPU process is still reported.
      // LocalVariables(Async) would send local variables' values, which can be too.
      ...defaults.filter(
        ({ name }) => name !== "SentryMinidump" && !name.startsWith("LocalVariables"),
      ),
      Sentry.startupTracingIntegration(),
    ],
    // No IP address, local variables' values, or HTTP headers, bodies and query strings.
    dataCollection: {
      userInfo: false,
      stackFrameVariables: false,
      cookies: false,
      httpHeaders: false,
      httpBodies: [],
      urlQueryParams: false,
    },
    // Traces are sent as spans, one by one, not as transactions.
    beforeSend: scrub,
    beforeSendSpan: scrub,
  });
  findStartupSpan();
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

/** The startup trace's root, while it's open; `undefined` when it's not sampled, or has ended. */
let startup: Sentry.Span | undefined;

/** The startup tracing integration doesn't hand its root span out, so it's caught as it starts. */
function findStartupSpan() {
  const client = Sentry.getClient();
  client?.on("spanStart", (span) => {
    const op = Sentry.spanToJSON(span).attributes[Sentry.SEMANTIC_ATTRIBUTE_SENTRY_OP];
    if (!startup && op === "app.start") startup = span;
  });
  client?.on("spanEnd", (span) => {
    if (span === startup) startup = undefined;
  });
}

/** Runs `work` as a part of the startup trace, while Gitto's starting; otherwise just runs it. */
export function traceStartup<T>(name: string, work: () => T): T {
  if (!startup) return work();
  return Sentry.startSpan({ name, op: "app.start.main", parentSpan: startup }, work);
}
