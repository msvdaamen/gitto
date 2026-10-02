import { defineConfig } from "vitest/config";

// `pnpm perf`: benchmarks on real repositories (see GITTO_PERF_REPOS in packages/git/perf). One
// file at a time, so they don't slow each other down.
export default defineConfig({
  test: {
    projects: ["packages/git/vitest.perf.config.ts"],
    fileParallelism: false,
  },
});
