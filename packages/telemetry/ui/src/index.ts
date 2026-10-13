import * as Sentry from "@sentry/electron/renderer";
import { init as solidInit } from "@sentry/solid";

import { startStartup } from "./startup";

export { captureException } from "@sentry/solid";
export { traceStartup } from "./startup";

/**
 * Catches the renderer's errors and times its part of starting up, for the main process to send:
 * it decides whether anything is, as the user's set it to, and how often startups are traced.
 */
export function initTelemetry({ enabled }: { enabled: boolean }) {
  if (!enabled) return;
  // Every startup's traced here; those the main process doesn't keep go no further than it.
  Sentry.init({ tracesSampleRate: 1 }, solidInit);
  startStartup();
}
