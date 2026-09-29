import { defineConfig, type Plugin } from "vite";

export default defineConfig(({ command }) => ({
  build: {
    target: "node24",
    // ESM output; Forge's default is CommonJS.
    lib: { entry: "src/main.ts", formats: ["es"], fileName: () => "main.js" },
  },
  plugins: command === "serve" ? [restartOnRebuild()] : [],
}));

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
