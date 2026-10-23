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
- `gitto-db` (`crates/db`): `packages/db`. Both apps share one `gitto.db` while both ship, so it
  embeds the migrations drizzle-kit makes in `apps/electron/migrations` and applies them exactly as
  Drizzle's migrator does: make migrations with `pnpm db:generate` only, never by hand in Rust.
- `gitto-repository` (`crates/repository`): `packages/repository`'s `server/`. Its
  `RepositoryService` is gitto-git's `RepositoryLookup`, so `GitApi::new(service)` opens the
  repositories added to Gitto.
- `gitto-ui` (`crates/ui`): what `apps/ui/src` works out without Solid or the DOM, for the app's
  screens: `git/*.ts` outside `git/queries/` in `src/git/`, `lib/format.ts`, and the logic of the
  hooks that had any in `src/hooks/`. It has no GPUI dependency.

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

Porting a TS file: keep its behavior, including error messages, and the reasoning in its doc
comments, which say why as much as what. Port its tests one for one, as `#[cfg(test)] mod tests`
in the same file, named after the TS test (`it("rejects unknown repositories")` →
`fn rejects_unknown_repositories`). Async tests are plain `#[test]` functions that run
`smol::block_on`. Async code doesn't use tokio: GPUI has its own executor.

Dependencies go in the root `Cargo.toml`'s `[workspace.dependencies]`, used with
`dep.workspace = true`. Check the crates before pushing (as CI does):

```sh
cargo fmt --all
cargo clippy --workspace --exclude app --all-targets --locked -- -D warnings
cargo test --workspace --exclude app --locked
```
