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
      integrations: [
        // Sentry.browserTracingIntegration({
        //   // Resources load from disk, and in development Vite serves every module as its own.
        //   ignoreResourceSpans: [
        //     "resource.script",
        //     "resource.link",
        //     "resource.css",
        //     "resource.img",
        //     "resource.other",
        //   ],
        // }),
      ],
      tracesSampleRate: 1.0,
    },
    solidInit,
  );
}
