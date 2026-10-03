import { defineProject } from "vitest/config";

export default defineProject({
  test: {
    name: "app-perf",
    environment: "node",
    include: ["perf/**/*.perf.ts"],
    disableConsoleIntercept: true,
  },
});
