import * as Sentry from "@sentry/electron/main";

Sentry.init({
  enabled: !!process.env.SENTRY_DSN,
  dsn: process.env.SENTRY_DSN,
  tracesSampleRate: 1.0,
  integrations: [Sentry.startupTracingIntegration()],
});
