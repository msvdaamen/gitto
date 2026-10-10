import { defineConfig, mergeConfig } from "vite";

import { sentrySourceMaps } from "./vite.sentry.ts";

export default mergeConfig(
  sentrySourceMaps(),
  defineConfig({
    build: {
      target: "node24",
      rollupOptions: {
        // Sandboxed preload scripts must be CommonJS; `.cjs` because this package is `type: module`.
        output: { entryFileNames: "[name].cjs" },
      },
    },
  }),
);
