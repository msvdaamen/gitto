import { resolve } from "node:path";

import tailwindcss from "@tailwindcss/vite";
import { tanstackRouter } from "@tanstack/router-plugin/vite";
import { defineConfig } from "vite";
import solid from "vite-plugin-solid";

export default defineConfig({
  plugins: [
    // Must come before the Solid plugin. Paths are absolute because Electron Forge
    // also loads this config from apps/electron (see vite.renderer.config.ts there).
    tanstackRouter({
      target: "solid",
      autoCodeSplitting: true,
      routesDirectory: resolve(import.meta.dirname, "src/routes"),
      generatedRouteTree: resolve(import.meta.dirname, "src/routeTree.gen.ts"),
    }),
    tailwindcss(),
    solid(),
  ],
  resolve: {
    alias: {
      "@": resolve(import.meta.dirname, "src"),
    },
  },
  // Whether Gitto is built to report to Sentry, which the renderer does through the main process
  // (see src/lib/sentry.ts).
  define: { "import.meta.env.SENTRY": JSON.stringify(!!process.env.SENTRY_DSN) },
  build: {
    target: "esnext",
  },
  // ES module workers: the diff viewer's highlighting worker loads each language as it's needed.
  worker: { format: "es" },
});
