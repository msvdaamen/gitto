// Builds the app into apps/electron/.vite, the way `electron-forge package` does before packaging,
// but without packaging it into out/: the driver runs the .vite build, and packaging copies the
// dependencies and Electron again on every rebuild. The targets come from forge.config.ts.
import * as fs from "node:fs";
import { createRequire } from "node:module";
import * as path from "node:path";

const APP_DIR = path.resolve(import.meta.dirname, "../../../apps/electron");
// Forge runs there, and the targets' config paths are relative to it.
process.chdir(APP_DIR);
const require = createRequire(path.join(APP_DIR, "package.json"));
const vite = require("vite");
const { default: ViteConfigGenerator } = require("@electron-forge/plugin-vite/dist/ViteConfig");

const loaded = await vite.loadConfigFromFile(
  { command: "build", mode: "production" },
  path.join(APP_DIR, "forge.config.ts"),
  APP_DIR,
  "silent",
);
const plugin = loaded?.config.plugins?.find((p) => p?.name === "vite");
if (!plugin) throw new Error("no Vite plugin in apps/electron/forge.config.ts");

const generator = new ViteConfigGenerator(plugin.config, APP_DIR, true);
const configs = [...(await generator.getBuildConfigs()), ...(await generator.getRendererConfig())];
fs.rmSync(path.join(APP_DIR, ".vite"), { recursive: true, force: true });
await Promise.all(
  configs.map((config) => vite.build({ configFile: false, logLevel: "error", ...config })),
);
console.log("built apps/electron/.vite");
