import { resolve } from "node:path";

import { mergeConfig } from "vite";

import uiConfig from "../ui/vite.config.ts";

// The renderer is the standalone UI app in apps/ui, built into this app's .vite folder.
export default mergeConfig(uiConfig, {
  root: resolve(import.meta.dirname, "../ui"),
  // Absolute asset paths so nested routes work when served from the app:// protocol.
  base: "/",
  // Forge enables this by default, but it breaks resolution with pnpm's symlinked node_modules.
  resolve: { preserveSymlinks: false },
  build: {
    outDir: resolve(import.meta.dirname, ".vite/renderer/main_window"),
    emptyOutDir: true,
  },
});
