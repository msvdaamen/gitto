import { defineConfig, type Plugin, type UserConfigFnObject } from "vite";

/**
 * Builds `src/<name>.ts` into `<name>.js` for a Node process of the app: the main process, or the
 * backend process it starts.
 */
export function nodeConfig(name: string): UserConfigFnObject {
  return defineConfig(({ command }) => ({
    build: {
      target: "node24",
      // ESM output; Forge's default is CommonJS.
      lib: { entry: `src/${name}.ts`, formats: ["es"], fileName: () => `${name}.js` },
      // Native modules can't be bundled; they're loaded from node_modules (see forge.config.ts).
      rollupOptions: { external: ["@parcel/watcher"] },
    },
    plugins: command === "serve" ? [restartOnRebuild()] : [],
  }));
}

/** Restarts Electron when the process is rebuilt (Forge's Vite plugin doesn't do this yet). */
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
