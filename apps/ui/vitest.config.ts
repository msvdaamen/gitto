import { defineProject, mergeConfig } from "vitest/config";

import viteConfig from "./vite.config.ts";

// Reuses the app's Vite config for the Solid JSX transform and the `@` alias. The Solid plugin
// also switches the test environment to jsdom.
export default mergeConfig(
  viteConfig,
  defineProject({
    test: {
      name: "ui",
    },
  }),
);
