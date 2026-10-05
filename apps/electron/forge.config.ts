import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { MakerSquirrel } from "@electron-forge/maker-squirrel";
import { MakerZIP } from "@electron-forge/maker-zip";
import { FusesPlugin } from "@electron-forge/plugin-fuses";
import { VitePlugin } from "@electron-forge/plugin-vite";
import type { ForgeConfig } from "@electron-forge/shared-types";
import { FuseV1Options, FuseVersion } from "@electron/fuses";

import { copyDependencies, findDependencies } from "./src/native-deps";
import { squirrelVersion } from "./src/squirrel-version";

/** Native modules: left out of the bundle (see vite.main.config.ts), so they're copied in. */
const NATIVE_DEPENDENCIES = ["@parcel/watcher"];

/** Set by CI at build time (see .github/workflows/release.yml), e.g. 1.2.4-nightly.20261005134259. */
const { version } = JSON.parse(readFileSync(new URL("package.json", import.meta.url), "utf8")) as {
  version: string;
};

const config: ForgeConfig = {
  packagerConfig: {
    // Native binaries can't be loaded from inside the archive.
    asar: { unpack: "**/*.node" },
    // Gitto.exe and Gitto.app/Contents/MacOS/Gitto, but a lowercase `gitto` on Linux, as commands
    // are there. Builds are made on the platform they're for (see packageAfterCopy).
    executableName: process.platform === "linux" ? "gitto" : "Gitto",
    // Extension is picked per platform (icon.ico on Windows, icon.icns on macOS).
    icon: "assets/icon",
    // The executable's version on Windows and the bundle's on macOS, which have to be numbers only,
    // so a nightly's is its release's (1.2.4). The app's own, from package.json, is the full one.
    appVersion: version.split("-")[0],
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
    // The NuGet package's id, which names the folder it's installed in (src/squirrel.ts has it too).
    // By default it's the scoped package name, which NuGet won't take. A nightly's version is made to
    // fit NuGet's and Squirrel's (see src/squirrel-version.ts); the installer's name has it whole.
    new MakerSquirrel({
      name: "Gitto",
      setupIcon: "assets/icon.ico",
      version: squirrelVersion(version),
    }),
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
