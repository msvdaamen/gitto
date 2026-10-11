import * as Sentry from "@sentry/electron/renderer";
import { init as solidInit } from "@sentry/solid";

export { startSpan } from "@sentry/solid";

Sentry.init(
  {
    integrations: [Sentry.browserTracingIntegration()],
    tracesSampleRate: 1.0,
  },
  solidInit,
);
