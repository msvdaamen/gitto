import { defineConfig } from "vitest/config";

// `pnpm test` runs every project in one Vitest process; each project has its own
// vitest.config.ts and can also be run on its own with `pnpm --filter <name> test`.
export default defineConfig({
  test: {
    projects: ["apps/*", "packages/git"],
  },
});
