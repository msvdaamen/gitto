import { basename } from "node:path";

import type { Worktree } from "./schema";

/**
 * How `worktree list --porcelain -z` is read: every line ends in a NUL, and an empty line ends
 * each worktree.
 */
export const WORKTREE_LIST_ARGS = ["worktree", "list", "--porcelain", "-z"];

/**
 * The worktrees in `worktree list --porcelain -z`'s output (see `WORKTREE_LIST_ARGS`), the main one
 * first; none is `current` yet (see `listWorktrees`).
 */
export function parseWorktreeList(output: string): Worktree[] {
  const worktrees: Worktree[] = [];
  for (const entry of output.split("\0\0")) {
    const lines = entry.split("\0").filter(Boolean);
    const path = lines.find((line) => line.startsWith("worktree "))?.slice("worktree ".length);
    if (path === undefined) continue;
    const attribute = (name: string): string | null => {
      const line = lines.find(
        (candidate) => candidate === name || candidate.startsWith(`${name} `),
      );
      return line === undefined ? null : line.slice(name.length + 1);
    };
    worktrees.push({
      path,
      name: basename(path),
      head: attribute("HEAD"),
      branch: attribute("branch"),
      main: worktrees.length === 0,
      bare: attribute("bare") !== null,
      current: false,
      locked: attribute("locked"),
      prunable: attribute("prunable"),
    });
  }
  return worktrees;
}
