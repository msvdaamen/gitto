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

- Every night, `main` is published as a **Gitto Nightly** pre-release, if the code changed since
  the last one; the newest 30 are kept. Its version follows the highest release tag, then says when
  it was built, in minutes from 1970: `1.2.4-nightly29853462` after `v1.2.3`, and
  `1.3.0-nightly29853462` after `v1.3.0-rc.1`, tagged `v1.2.4-nightly29853462` as releases are.
  Every build has the same version, the Windows installer's package too.
- Running the **Release** workflow on `main` (Actions → Release → Run workflow) builds a nightly
  now, or releases `main` with the last release's version bumped: `auto` bumps what the commits
  since then call for (a breaking change the major, a `feat` the minor, else the patch), or pick
  `patch` (1.2.3 to 1.2.4), `minor` (1.3.0) or `major` (2.0.0). Pre-releases are left out, so after
  `v1.3.0-rc.1` a `minor` release is still 1.3.0. Once it's built, it's tagged and published.
- Pushing a tag publishes that version, which is how to make a pre-release, like `v1.3.0-beta.1`;
  its pre-release part has to start with a letter.

```sh
gh workflow run release.yml -f build=auto   # or nightly, patch, minor or major
git tag v1.3.0-beta.1 && git push origin v1.3.0-beta.1
```

### Changelog

Pull requests are squashed, titled with [Conventional Commits](https://www.conventionalcommits.org):
`feat: add worktrees`, `fix(diff): keep the scroll`, `feat!: …` for a breaking change. The **PR
title** workflow checks it. A `Changelog: …` line in the description tells users more than the
title does.

Each build lists the commits since the build before it on its channel (the last nightly, or the
release before), adds them to that build's `changelog.json` and publishes it with its files; its
release notes are the same changes, grouped. `packages/release/src/cli.ts` makes both. Every type
is kept; Gitto shows users what's new, fixed or faster (`feat`, `fix`, `perf`).

The builds package their `changelog.json` (`apps/electron/forge.config.ts`). When Gitto starts on a
newer version than the last one the user saw, **What's new** lists what changed in each version
since, and clicking Gitto's version in the footer lists every version. The last version seen is in
`last-seen-version`, in Gitto's user data folder; on the first run there's nothing to show. To try it
locally, put a `changelog.json` in `apps/electron` (ignored by git) and build with a version, as
`npm pkg set version=…` there does.

The builds aren't code-signed, so macOS and Windows warn before opening them.

### Updates

Gitto updates itself on Windows and macOS (`apps/electron/src/updater.ts`), from its own channel: a
nightly only to the next nightly, a release only to the next release. It checks when it starts and
every hour, downloads a newer version in the background, and offers to restart into it in the footer;
otherwise it's installed the next time Gitto starts.

It finds the newest on its channel in [GitHub's list of releases](https://api.github.com/repos/msvdaamen/gitto/releases),
leaving out pre-releases other than nightlies (so a `v1.3.0-beta.1` isn't offered), and updates from
that release's files:

- `RELEASES` and the `.nupkg`, which Squirrel updates from on Windows.
- `update-darwin-arm64.json`, which points Squirrel.Mac to the zip. Squirrel.Mac only installs a
  code-signed app, so updates on macOS need the builds to be signed.

On Linux, Gitto doesn't update itself: pacman does, once it's on the AUR.

## Adding routes

Create a file in `apps/ui/src/routes/`. The TanStack Router Vite plugin regenerates
`routeTree.gen.ts` automatically.

## Editor (Zed)

Install the **Oxc** (oxlint + oxfmt) and **TypeScript Language Server** (tsgo, for TypeScript 7)
extensions. Project settings live in `.zed/settings.json`.
