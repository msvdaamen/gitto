import { readFileSync } from "node:fs";

import { sentryVitePlugin } from "@sentry/vite-plugin";
import type { UserConfig } from "vite";

/** Set by CI at build time (see .github/workflows/release.yml), e.g. 1.2.4-nightly29853462. */
const { version } = JSON.parse(readFileSync(new URL("package.json", import.meta.url), "utf8")) as {
  version: string;
};

/**
 * With a SENTRY_AUTH_TOKEN, as release builds have (see .github/workflows/release.yml), makes source
 * maps and uploads them to Sentry, which then shows errors' stack traces in the source. They're
 * left out of the packaged app (see forge.config.ts).
 */
export function sentrySourceMaps(): UserConfig {
  const { SENTRY_AUTH_TOKEN, SENTRY_ORG, SENTRY_PROJECT } = process.env;
  if (!SENTRY_AUTH_TOKEN) return {};
  return {
    // Not linked from the files they map, as nothing loads them.
    build: { sourcemap: "hidden" },
    plugins: [
      sentryVitePlugin({
        authToken: SENTRY_AUTH_TOKEN,
        org: SENTRY_ORG,
        project: SENTRY_PROJECT,
        // The release the main process reports as, by default (see src/sentry.ts).
        release: { name: `Gitto@${version}` },
        telemetry: false,
      }),
    ],
  };
}
