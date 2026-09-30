import { defineProject } from "vitest/config";

export default defineProject({
  test: {
    name: "git",
    environment: "node",
    setupFiles: ["./vitest.setup.ts"],
  },
});
