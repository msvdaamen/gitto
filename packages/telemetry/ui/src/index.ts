import * as Sentry from "@sentry/electron/renderer";
import { init as solidInit } from "@sentry/solid";

export { captureException } from "@sentry/solid";

/**
 * Catches the renderer's errors for the main process to send: it decides whether anything is, as
 * the user's set it to.
 */
export function initTelemetry({ enabled }: { enabled: boolean }) {
  if (!enabled) return;
  Sentry.init({}, solidInit);
}
