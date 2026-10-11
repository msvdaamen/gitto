import * as Sentry from "@sentry/electron/renderer";
import { init as solidInit } from "@sentry/solid";

export { startSpan, captureException } from "@sentry/solid";

type Init = {
  enabled?: boolean;
};

export function initSentry({ enabled }: Init) {
  Sentry.init(
    {
      enabled,
      integrations: [Sentry.browserTracingIntegration()],
      tracesSampleRate: 1.0,
    },
    solidInit,
  );
}
