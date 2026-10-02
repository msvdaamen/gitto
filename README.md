# Gitto

Electron desktop app with a SolidJS + TanStack Router UI.

## Structure

```
apps/
  ui/        SolidJS + TanStack Router (file-based routes), standalone Vite app
  electron/  Electron main, backend + preload, built and packaged with Electron Forge + its Vite plugin
packages/
  git/         Runs git and watches repositories; one folder per feature (status, history, …)
  repository/  The repositories added to Gitto
  system/      Native OS capabilities (dialogs)
  rpc/         The typed API between the UI and the app's processes
  db/          SQLite database
```

The app runs in three processes. The main process owns the window and native dialogs. The backend
(an Electron utility process, `apps/electron/src/backend.ts`) owns the database, runs git and
watches files, so none of that can keep the main process, which routes the window's input, busy.
The UI talks to each over a MessagePort of its own (`packages/rpc`).

Electron Forge's Vite plugin builds everything: main, backend and preload via `apps/electron/vite.*.config.ts`,
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
| `pnpm make`      | Create distributables (zip, deb, squirrel)                  |
| `pnpm typecheck` | Typecheck all apps                                          |
| `pnpm test`      | Run all tests with Vitest                                   |
| `pnpm lint`      | Lint with oxlint                                            |
| `pnpm format`    | Format with oxfmt                                           |

## Adding routes

Create a file in `apps/ui/src/routes/`. The TanStack Router Vite plugin regenerates
`routeTree.gen.ts` automatically.

## Editor (Zed)

Install the **Oxc** (oxlint + oxfmt) and **TypeScript Language Server** (tsgo, for TypeScript 7)
extensions. Project settings live in `.zed/settings.json`.
