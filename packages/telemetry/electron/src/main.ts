import * as Sentry from "@sentry/electron/main";

type Init = {
  dsn: string | undefined;
  enabled?: boolean;
  version: string;
  isPackaged: boolean;
  debug?: boolean;
};

export function initSentry({ enabled, dsn, version, isPackaged, debug }: Init) {
  Sentry.init({
    enabled: enabled !== undefined ? enabled : !!dsn,
    dsn,
    environment: environment(version, isPackaged),
    tracesSampleRate: 1.0,
    integrations: [Sentry.startupTracingIntegration()],
    debug: debug ?? false,
  });
}

function environment(version: string, isPackaged: boolean) {
  if (!isPackaged) return "development";
  return version.includes("-nightly") ? "nightly" : "production";
}
