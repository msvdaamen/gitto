# Gitto

Electron desktop app with a SolidJS + TanStack Router UI.

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
| `pnpm make`      | Create distributables (Windows installer, macOS zip)        |
| `pnpm make:arch` | Create an Arch Linux package (needs `makepkg`)              |
| `pnpm typecheck` | Typecheck all apps                                          |
| `pnpm test`      | Run all tests with Vitest                                   |
| `pnpm lint`      | Lint with oxlint                                            |
| `pnpm format`    | Format with oxfmt                                           |

## Releases

`.github/workflows/release.yml` builds Gitto for Windows (Squirrel installer), macOS (zip) and Arch
Linux (`.pkg.tar.zst`, made by `apps/electron/arch/PKGBUILD`) and publishes it to GitHub Releases.
The version comes from git tags; `package.json`'s stays at `0.0.0` and is set at build time.

Install the Arch package with `sudo pacman -U gitto-*.pkg.tar.zst`.

- Every merge to `main` replaces the **Nightly** pre-release. Its version is the next patch after the
  last `v*` tag, e.g. `1.2.4-nightly.20261005.42` after `v1.2.3` (date, then the workflow's run
  number).
- Pushing a `v1.3.0` tag publishes **Gitto 1.3.0** with generated notes. A tag with a pre-release
  part, like `v1.3.0-beta.1`, is published as a pre-release.

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
