import { defineConfig } from "vite";

export default defineConfig({
  build: {
    target: "node24",
    rollupOptions: {
      // Sandboxed preload scripts must be CommonJS; `.cjs` because this package is `type: module`.
      output: { entryFileNames: "[name].cjs" },
    },
  },
});
