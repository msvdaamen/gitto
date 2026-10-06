// Repositories with conflicts, for the tests of resolving them and of what's under way.
import { spawnSync } from "node:child_process";
import { rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { createRepo, git } from "./fixtures";

/** Files to write, by path: their contents, or `null` to delete one. */
export type Files = Record<string, string | Buffer | null>;

/** Runs git in `cwd`, which may fail, as a merge that conflicts does; returns its exit code. */
export function tryGit(cwd: string, ...args: string[]): number {
  return spawnSync("git", args, { cwd, encoding: "utf8" }).status ?? -1;
}

/** Writes `files` in the repository at `path`, and commits them with `message`. */
export function commitFiles(path: string, files: Files, message: string): void {
  for (const [file, contents] of Object.entries(files)) {
    if (contents === null) rmSync(join(path, file));
    else writeFileSync(join(path, file), contents);
  }
  git(path, "add", "-A");
  git(path, "commit", "-qm", message);
}

/**
 * A repository, which `repos` opens as `name`, where `main` and `side` both changed what `base`
 * committed: `main` to `ours`, `side` to `theirs`. `main` is checked out, with `conflictStyle` set
 * if given. Returns its path.
 */
export function createDivergedRepo(
  name: string,
  { base, ours, theirs }: { base: Files; ours: Files; theirs: Files },
  conflictStyle?: "merge" | "diff3" | "zdiff3",
): string {
  const path = createRepo(name);
  if (conflictStyle) git(path, "config", "merge.conflictStyle", conflictStyle);
  commitFiles(path, { README: "readme\n", ...base }, "base");
  git(path, "checkout", "-qb", "side");
  commitFiles(path, theirs, "theirs");
  git(path, "checkout", "-q", "main");
  commitFiles(path, ours, "ours");
  return path;
}

/** `f.txt` with its second line changed on both sides. */
export const TEXT_CONFLICT = {
  base: { "f.txt": "one\ntwo\nthree\n" },
  ours: { "f.txt": "one\nours\nthree\n" },
  theirs: { "f.txt": "one\ntheirs\nthree\n" },
};

/** A repository merging `side` into `main`, stopped at `f.txt`'s conflict (see `TEXT_CONFLICT`). */
export function createMergeConflict(
  name: string,
  conflictStyle?: "merge" | "diff3" | "zdiff3",
  sides: { base: Files; ours: Files; theirs: Files } = TEXT_CONFLICT,
): string {
  const path = createDivergedRepo(name, sides, conflictStyle);
  tryGit(path, "merge", "-q", "side");
  return path;
}
