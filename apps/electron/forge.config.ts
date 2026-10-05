import { fileURLToPath } from "node:url";

import { MakerSquirrel } from "@electron-forge/maker-squirrel";
import { MakerZIP } from "@electron-forge/maker-zip";
import { FusesPlugin } from "@electron-forge/plugin-fuses";
import { VitePlugin } from "@electron-forge/plugin-vite";
import type { ForgeConfig } from "@electron-forge/shared-types";
import { FuseV1Options, FuseVersion } from "@electron/fuses";

import { copyDependencies, findDependencies } from "./src/native-deps";

/** Native modules: left out of the bundle (see vite.main.config.ts), so they're copied in. */
const NATIVE_DEPENDENCIES = ["@parcel/watcher"];

const config: ForgeConfig = {
  packagerConfig: {
    // Native binaries can't be loaded from inside the archive.
    asar: { unpack: "**/*.node" },
    // The package name is scoped (@gitto/electron), so set a plain binary name explicitly.
    executableName: "gitto",
    // Extension is picked per platform (icon.ico on Windows, icon.icns on macOS).
    icon: "assets/icon",
    // Copied to Resources/migrations; the main process applies them on startup.
    extraResource: ["migrations"],
  },
  // Prebuilt for Node-API, which Electron supports as is; rebuilding would need a compiler.
  rebuildConfig: { ignoreModules: NATIVE_DEPENDENCIES },
  hooks: {
    async packageAfterCopy(_config, buildPath, _electronVersion, platform, arch) {
      const packages = await findDependencies(
        NATIVE_DEPENDENCIES,
        fileURLToPath(new URL(".", import.meta.url)),
      );
      // Only this machine's prebuilt binary is installed, so another platform would crash on start.
      const binary = `@parcel/watcher-${platform}-${arch}`;
      // Exactly that name, or with a libc suffix: `linux-arm` isn't `linux-arm64-glibc`.
      if (![...packages.keys()].some((name) => name === binary || name.startsWith(`${binary}-`))) {
        throw new Error(
          `${binary} isn't installed, so Gitto can't be packaged for ${platform}-${arch} here.`,
        );
      }
      await copyDependencies(packages, buildPath);
    },
  },
  makers: [
    new MakerSquirrel({ setupIcon: "assets/icon.ico" }),
    new MakerZIP({}, ["darwin"]),
    // Linux is packaged for Arch by arch/PKGBUILD instead (`pnpm make:arch`).
  ],
  plugins: [
    new VitePlugin({
      build: [
        { entry: "src/main.ts", config: "vite.main.config.ts", target: "main" },
        { entry: "src/preload.ts", config: "vite.preload.config.ts", target: "preload" },
      ],
      renderer: [{ name: "main_window", config: "vite.renderer.config.ts" }],
    }),
    new FusesPlugin({
      version: FuseVersion.V1,
      [FuseV1Options.RunAsNode]: false,
      [FuseV1Options.EnableCookieEncryption]: true,
      [FuseV1Options.EnableNodeOptionsEnvironmentVariable]: false,
      [FuseV1Options.EnableNodeCliInspectArguments]: false,
      [FuseV1Options.EnableEmbeddedAsarIntegrityValidation]: true,
      [FuseV1Options.OnlyLoadAppFromAsar]: true,
    }),
  ],
};

export default config;
