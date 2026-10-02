import { defineProject, mergeConfig } from "vitest/config";

import config from "./vitest.config.ts";

export default mergeConfig(
  config,
  defineProject({
    test: {
      name: "git-perf",
      include: ["perf/**/*.perf.ts"],
      disableConsoleIntercept: true,
      testTimeout: 30 * 60 * 1000,
    },
  }),
);
