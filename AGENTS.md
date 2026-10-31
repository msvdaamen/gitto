# Agent notes

What to know when working on or reviewing Gitto that the code doesn't say by itself.

## Git version

Gitto requires a recent git: **2.41 or newer**. Use whatever that version has, and don't add
workarounds or fallbacks for older ones. In reviews, don't flag a git command or option for being
missing from older git, unless it's newer than 2.41.

## Commit and pull request titles

Pull requests are squashed into one commit titled as the pull request, and Gitto's changelog is
made from those titles, so title each with [Conventional Commits](https://www.conventionalcommits.org):
`<type>(<optional scope>): <description>`, lowercase, saying what changes for the user.

- `feat` for something users can do that they couldn't, `fix` for something that works now, `perf`
  for something faster. These are what users see in the app's changelog.
- `refactor`, `docs`, `style`, `test`, `build`, `ci`, `chore` or `revert` for the rest, which only
  the release notes list.
- `!` after the type (`feat!: …`) for a breaking change.

A `Changelog: …` line in the description, for users, explains more than the title can. The PR
title workflow checks titles with `node packages/release/src/cli.ts check-title "<title>"`.

## Rust

Gitto is moving to Rust, with a GPUI ([gpui-kit](https://crates.io/crates/gpui-kit)) UI in
`apps/app`, which calls the crates in `crates/` directly (no oRPC):

- `gitto-git` (`crates/git`): `packages/git`.
- `gitto-db` (`crates/db`): Gitto's SQLite database, `gitto-app.db`: a file of its own, not the
  Electron app's `gitto.db` (see `crates/db/src/paths.rs`). Its schema is its own, not the Electron
  app's Drizzle one: change it by adding a migration to `crates/db/migrations` and `MIGRATIONS`,
  never by editing one that's shipped.
- `gitto-repository` (`crates/repository`): `packages/repository`'s `server/`. Its
  `RepositoryService` is gitto-git's `RepositoryLookup`, so `GitApi::new(service)` opens the
  repositories added to Gitto.
- `gitto-ui-logic` (`crates/ui-logic`): what `apps/ui/src` works out without Solid or the DOM,
  for the app's screens: `git/*.ts` outside `git/queries/` in `src/git/`, the model of a diff
  (`components/repository/diff/*.ts`) in `src/diff/`, and `lib/format.ts`. It has no GPUI
  dependency. The hooks are the app's `state/` (in `gitto-app-data`).

In `gitto-git`:

- `src/core/`: `packages/git/src/core/` (running git, `Repo`/`GitRepos`, the errors, git dirs, the
  version check, tracing). `src/input.rs` and `src/schema.rs`: `input.ts` and `schema.ts`.
- `src/api/`: what the oRPC layer did (`middleware.ts`, the routers, contracts and handlers, and
  `server.ts`'s errors): `GitApi` opens a repository by its id, after checking git, and turns
  errors into the `ApiError` the UI shows. A contract's input checks are the types the features
  take.
- `src/features/<feature>/`: `packages/git/src/features/<feature>/`, one submodule per TS file
  (`commands.ts` → `commands.rs`, `in-use.ts` → `in_use.rs`), declared in that feature's `mod.rs`.
- `src/test_support/`: `packages/git/src/test/` (real repositories in a temp dir), for tests only.

`apps/app` is laid out as `apps/ui/src` is, one module per TS file, in three crates: the app
(`apps/app/src`), `gitto-app-data` (`apps/app/data`: `data/`, `services/`, `state/`) and
`gitto-app-kit` (`apps/app/kit`: `ui/`, `theme/`). The app builds on both; the kit builds on GPUI
Kit (and `serde_json`) alone, not the data crate: it's given what to show (an error's message, say).
Neither knows of the app's pages or areas.

- `views/`: the pages (`routes/`) and the window around them. `views/repository.rs` lays out a
  repository's areas and wires them together: the selection, the file on show, the watcher.
- `components/<area>/`: what pages are made of (`components/`); the primitives are the kit's
  `ui/` (`components/ui/`). An area that keeps state is an entity, made with a repository's id, told of
  another with `set_repository`, and talking back through its events.
- `data/`: the data layer, in place of TanStack Query: a `Repository` entity per repository (in
  the `Repositories` global) holds what's loaded of it as fields and makes the changes to it, a
  view shows parts of it with `Shown`, `AppData` holds what isn't a repository's, and
  `RepositoryWatcher` reloads what changes on disk. Git and the other crates run off the main
  thread.
- `state/` (what outlives a view, and the hooks GPUI needs as state), `services/` (what's started
  at launch, in the `Services` global), and `theme/` (`index.css`).

GPUI tests can't run git: a view's test gives the repository's entity what it shows first
(`components/repository/test_support.rs`), and fakes what changes it with a `RepositoryGit`.

The app's binary is `gitto`, and the OS knows it by the id and name in `apps/app/src/identity.rs`:
Windows takes its icon and name from the .exe (`apps/app/build.rs`), and on Linux a build that isn't
installed writes itself a desktop entry (`NoDisplay`) in `$XDG_DATA_HOME`, for the task bar.

Dev builds optimize the dependencies (`[profile.dev.package."*"]` in the root `Cargo.toml`): GPUI
is many times slower without, so the first build takes longer, and the app's speed is only judged
in such a build or a `--release` one.

Building the app on Linux takes libxkbcommon, Wayland, fontconfig and FreeType's headers
(`libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libfontconfig-dev libfreetype-dev`). To run
it headless, start a display with `Xvfb :99 -screen 0 1400x900x24 &`, and run `cargo run -p app`
with `DISPLAY=:99` and `GITTO_USER_DATA_DIR` set to a folder of its own. There's no folder picker
without a desktop portal, so add repositories as rows of that folder's `gitto-app.db`
(`repositories`: a UUIDv7 `id`, `name`, `path` and `position`), e.g. one
`.claude/skills/run-app/make-demo-repo.sh` made. Screenshot with `import -window root shot.png` and
drive it with `xdotool`. Software rendering needs Mesa's Vulkan driver (lavapipe).

Porting a TS file: keep its behavior, including error messages, and the reasoning in its doc
comments, which say why as much as what. Port its tests one for one, as `#[cfg(test)] mod tests`
in the same file, or, once they're many, in a `tests.rs` beside it (`commit_form.rs`'s in
`commit_form/tests.rs`), named after the TS test (`it("rejects unknown repositories")` →
`fn rejects_unknown_repositories`). Async tests are plain `#[test]` functions that run
`smol::block_on`. Async code doesn't use tokio: GPUI has its own executor.

Dependencies go in the root `Cargo.toml`'s `[workspace.dependencies]`, used with
`dep.workspace = true`. Check the workspace before pushing (as CI does):

```sh
cargo fmt --all
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy --workspace --locked -- -D warnings
cargo test --workspace --locked
```
