import { defineConfig, mergeConfig, type Plugin } from "vite";

import { sentrySourceMaps } from "./vite.sentry.ts";

export default defineConfig(({ command }) =>
  mergeConfig(sentrySourceMaps(), {
    // Where Gitto reports to (see src/sentry.ts); a build without one reports nothing.
    define: { SENTRY_DSN: JSON.stringify(process.env.SENTRY_DSN ?? "") },
    build: {
      target: "node24",
      // ESM output; Forge's default is CommonJS.
      lib: { entry: "src/main.ts", formats: ["es"], fileName: () => "main.js" },
      // Native modules can't be bundled; they're loaded from node_modules (see forge.config.ts).
      rollupOptions: { external: ["@parcel/watcher"] },
    },
    plugins: command === "serve" ? [restartOnRebuild()] : [],
  }),
);

/** Restarts Electron when the main process is rebuilt (Forge's Vite plugin doesn't do this yet). */
function restartOnRebuild(): Plugin {
  let firstBuild = true;
  return {
    name: "gitto:restart-electron",
    closeBundle() {
      if (firstBuild) {
        firstBuild = false;
        return;
      }
      // Same as typing `rs` in the `electron-forge start` terminal.
      process.stdin.emit("data", "rs");
    },
  };
}
