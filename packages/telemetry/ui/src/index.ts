import * as Sentry from "@sentry/electron/renderer";

export { captureException } from "@sentry/electron/renderer";

/**
 * Catches the renderer's errors for the main process to send: it decides whether anything is, as
 * the user's set it to.
 */
export function initTelemetry({ enabled }: { enabled: boolean }) {
  if (!enabled) return;
  Sentry.init({});
}
