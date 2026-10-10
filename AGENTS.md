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
