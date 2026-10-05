# Gitto

Electron desktop app with a SolidJS + TanStack Router UI.

To install it on Windows, macOS or Arch Linux, see [Installing Gitto](docs/install.md).

## Structure

```
apps/
  ui/        SolidJS + TanStack Router (file-based routes), standalone Vite app
  electron/  Electron main + preload, built and packaged with Electron Forge + its Vite plugin
```

Electron Forge's Vite plugin builds everything: main and preload via `apps/electron/vite.*.config.ts`,
and the renderer via `apps/electron/vite.renderer.config.ts`, which reuses `apps/ui/vite.config.ts`.

In development the renderer is served by the Vite dev server. In production it is served from a
custom `app://` protocol (see `apps/electron/src/main.ts`) so the router can use normal browser
history URLs.

## Scripts

| Command          | Description                                                 |
| ---------------- | ----------------------------------------------------------- |
| `pnpm dev`       | Start the app (Vite HMR; type `rs` or edit main to restart) |
| `pnpm dev:ui`    | Run only the UI in the browser                              |
| `pnpm package`   | Package the app into `apps/electron/out`                    |
| `pnpm make`      | Create distributables on Windows and macOS                  |
| `pnpm make:arch` | Create an Arch Linux package; Linux has no `pnpm make`      |
| `pnpm typecheck` | Typecheck all apps                                          |
| `pnpm test`      | Run all tests with Vitest                                   |
| `pnpm lint`      | Lint with oxlint                                            |
| `pnpm format`    | Format with oxfmt                                           |

## Releases

`.github/workflows/release.yml` builds Gitto for Windows (Squirrel installer), macOS (zip) and Arch
Linux (`.pkg.tar.zst`, made by `apps/electron/arch/PKGBUILD`) and publishes it to GitHub Releases.
The version comes from git tags; `package.json`'s stays at `0.0.0` and is set at build time.

How to install each build is in [Installing Gitto](docs/install.md).

- Every merge to `main` updates the **Gitto Nightly** pre-release. Its version follows the highest `v*`
  tag: `1.2.4-nightly.20261005.42` after `v1.2.3` (the date, then the workflow's run number), and
  `1.3.0-rc.1.nightly.20261005.42` after `v1.3.0-rc.1`.
- Pushing a `v1.3.0` tag publishes **Gitto 1.3.0** with generated notes. A tag with a pre-release
  part, like `v1.3.0-beta.1`, is published as a pre-release; that part has to start with a letter.

```sh
git tag v1.3.0 && git push origin v1.3.0
```

The builds aren't code-signed, so macOS and Windows warn before opening them.

## Adding routes

Create a file in `apps/ui/src/routes/`. The TanStack Router Vite plugin regenerates
`routeTree.gen.ts` automatically.

## Editor (Zed)

Install the **Oxc** (oxlint + oxfmt) and **TypeScript Language Server** (tsgo, for TypeScript 7)
extensions. Project settings live in `.zed/settings.json`.
