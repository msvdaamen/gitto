import { consoleLoggingIntegration, init } from "@sentry/electron/renderer";
import { init as solidInit } from "@sentry/solid";
import { tanstackRouterBrowserTracingIntegration } from "@sentry/solid/tanstackrouter";
import type { AnyRouter } from "@tanstack/solid-router";

export { captureException } from "@sentry/electron/renderer";

/**
 * Reports the renderer's errors, the warnings and errors it logs, and traces of loading its pages
 * to Sentry. They're sent through the main process, which says where to and as which release (see
 * apps/electron/src/sentry.ts); a build without Sentry, or the UI in a browser, sends nothing.
 */
export function initSentry(router: AnyRouter) {
  if (!import.meta.env.SENTRY || !window.electron) return;
  init(
    {
      // As in the main process, whose startup trace this renderer's first page load is part of.
      tracesSampleRate: 1,
      integrations: [
        tanstackRouterBrowserTracingIntegration(router),
        consoleLoggingIntegration({ levels: ["warn", "error"] }),
      ],
    },
    solidInit,
  );
}
