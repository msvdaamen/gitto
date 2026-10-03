import { playwright } from "@vitest/browser-playwright";
import { defineConfig, mergeConfig } from "vitest/config";

import viteConfig from "./vite.config.ts";

// `pnpm perf:ui`: how long the renderer takes to draw its frames while someone scrolls, clicks and
// types, on a made-up repository (see perf/rpc.ts). jsdom does no layout or painting, so these run
// in a real Chromium, headless. It needs `pnpm --filter @gitto/ui exec playwright install chromium`
// once.
export default mergeConfig(
  viteConfig,
  defineConfig({
    test: {
      name: "ui-perf",
      include: ["perf/**/*.perf.tsx"],
      testTimeout: 5 * 60 * 1000,
      // The frame times are printed once each file's scenarios are done.
      silent: false,
      browser: {
        enabled: true,
        headless: true,
        provider: playwright(),
        // Electron's window size (see apps/electron/src/main.ts).
        instances: [{ browser: "chromium", viewport: { width: 1200, height: 800 } }],
        commands: await import("./perf/commands.ts"),
        screenshotFailures: false,
      },
    },
  }),
);
