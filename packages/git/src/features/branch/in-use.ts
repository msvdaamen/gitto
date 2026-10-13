import { readdir, readFile } from "node:fs/promises";
import { join } from "node:path";

import type { GitCommand } from "../../core/repo";

/** What the file at `path` says, trimmed; `""` if it isn't there. */
function read(path: string): Promise<string> {
  return readFile(path, "utf8").then(
    (text) => text.trim(),
    () => "",
  );
}

/**
 * The local branches git won't delete, nor Gitto pull without checking them out, by their full
 * names, each with why and what to do about it: checked out in a worktree, or being rebased or
 * bisected in one, which detaches its HEAD meanwhile. Read from each worktree's git directory,
 * where git keeps those, as it does.
 */
export async function branchesInUse(
  run: GitCommand,
): Promise<Map<string, { state: string; fix: string }>> {
  const [heads, common] = await Promise.all([
    run(["for-each-ref", "--format=%(refname)%00%(worktreepath)", "refs/heads/"]),
    run(["rev-parse", "--path-format=absolute", "--git-common-dir"]),
  ]);
  const inUse = new Map<string, { state: string; fix: string }>();
  for (const line of heads.split("\n")) {
    const [local, worktree] = line.split("\0");
    if (local && worktree) {
      inUse.set(local, {
        state: `is checked out in ${worktree}`,
        fix: "Switch to another branch there first.",
      });
    }
  }

  const commonDir = common.trim();
  const linked = await readdir(join(commonDir, "worktrees")).catch(() => []);
  const dirs = [commonDir, ...linked.map((id) => join(commonDir, "worktrees", id))];
  const states = await Promise.all(
    dirs.map((dir) =>
      Promise.all([
        read(join(dir, "rebase-merge", "head-name")),
        read(join(dir, "rebase-apply", "head-name")),
        read(join(dir, "BISECT_START")),
      ]),
    ),
  );
  for (const [rebaseMerge, rebaseApply, bisect] of states) {
    for (const rebased of [rebaseMerge, rebaseApply]) {
      if (rebased.startsWith("refs/heads/") && !inUse.has(rebased)) {
        inUse.set(rebased, { state: "is being rebased", fix: "Finish or abort the rebase first." });
      }
    }
    // The branch's short name; a commit's SHA when bisecting started detached.
    const bisected = `refs/heads/${bisect}`;
    if (bisect && !inUse.has(bisected)) {
      inUse.set(bisected, { state: "is being bisected", fix: "Stop bisecting first." });
    }
  }
  return inUse;
}
