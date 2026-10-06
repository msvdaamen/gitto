import { isAbsolute, relative, sep } from "node:path";

import type { GitDirs } from "../../core/git-dirs";
import type { GitDirChange } from "./schema";

/**
 * Files in a worktree's own git directory: HEAD (and MERGE_HEAD etc.), operations in progress, and
 * its refs when they're stored in a reftable (`git init --ref-format=reftable`).
 */
const WORKTREE_STATE = /^([A-Z_]*HEAD|(rebase-merge|rebase-apply|sequencer|reftable)(\/.*)?)$/;
/**
 * Files in the shared git directory: branches, tags and remotes (as files, packed, or in a
 * reftable), and the config (upstreams).
 */
const SHARED_STATE = /^((refs|reftable)(\/.*)?|packed-refs|config)$/;

/** What a change to `path`, in the git directory, means for the UI; `undefined` if nothing. */
export function classify(dirs: GitDirs, path: string): GitDirChange | undefined {
  // Written to while git works, then renamed onto the real file; that's the change that counts.
  if (path.endsWith(".lock")) return undefined;

  const own = inside(dirs.gitDir, path);
  if (own === "index") return "index";
  if (own !== undefined && WORKTREE_STATE.test(own)) return "refs";

  const shared = inside(dirs.commonDir, path);
  if (shared !== undefined && SHARED_STATE.test(shared)) return "refs";
  return undefined;
}

/** `path` relative to `dir` and `/`-separated, or `undefined` if it's not inside `dir`. */
export function inside(dir: string, path: string): string | undefined {
  const rel = relative(dir, path);
  if (!rel || rel === ".." || rel.startsWith(`..${sep}`) || isAbsolute(rel)) return undefined;
  return rel.split(sep).join("/");
}
