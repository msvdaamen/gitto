import { defineProject, mergeConfig } from "vitest/config";

import viteConfig from "./vite.config.ts";

// Reuses the app's Vite config for the Solid JSX transform and the `@` alias. The Solid plugin
// also switches the test environment to jsdom.
export default mergeConfig(
  viteConfig,
  defineProject({
    test: {
      name: "ui",
      // One jsdom per worker, which each file gets a fresh context of, rather than one per file:
      // making a jsdom took half the run (31s rather than 11s for the UI's tests). Files stay
      // isolated from each other, as `isolate: false` wouldn't keep them (the query client is a
      // module singleton). The vm context has no web streams; vitest.setup.ts adds them.
      pool: "vmThreads",
      setupFiles: ["./vitest.setup.ts"],
    },
  }),
);
