import { realpath } from "node:fs/promises";
import { resolve } from "node:path";

import { GitError, RepositoryChangedError, WorktreeRemovalBlockedError } from "../../core/errors";
import type { GitCommand, Repo } from "../../core/repo";
import { isRef, trackingBranch } from "../branch/commands";
import { parseRefName } from "../refs/parse";
import { parseWorktreeList, WORKTREE_LIST_ARGS } from "./parse";
import type { Worktree } from "./schema";

/**
 * The worktrees: the main one first, then the linked ones by their paths, with the one the
 * repository was opened as marked `current`.
 */
export async function listWorktrees(repo: Repo, signal?: AbortSignal): Promise<Worktree[]> {
  return (await listed(repo.path, (args) => repo.read(args, { signal }))).worktrees;
}

/**
 * Checks the branch `branch` (a full ref name) out in a new worktree at `folder` (an absolute
 * path, or one relative to the repository), which must be empty or not there yet. With
 * `newBranch`, a branch of that name is created from it and checked out there instead, tracking
 * it if it's a remote one. For a remote branch without one, that's the local branch tracking it,
 * or a new one that does, named after it without the remote (see `trackingBranch`). Git refuses
 * a branch that's checked out in a worktree already, the main one included, and a folder that's
 * in use, saying so.
 */
export async function addWorktree(
  repo: Repo,
  folder: string,
  branch: string,
  newBranch?: string,
): Promise<void> {
  const target = parseRefName(branch);
  if (!target || target.kind === "tag") throw new Error(`${branch} isn't a branch.`);
  const path = resolve(repo.path, folder);
  // Without the hint pointing to `git help check-ref-format` after an invalid name.
  const options = { config: ["advice.refSyntax=false"] };
  // One write, so nothing else changes the branches halfway.
  await repo.exclusive(async (run) => {
    // That ref itself, rather than git reading `branch` as a revision, like `refs/heads/main~3`.
    if (!(await isRef(run, branch))) throw new Error(`${branch} isn't a branch.`);
    const add = (...args: string[]) => run(["worktree", "add", "--quiet", ...args], options);
    try {
      if (newBranch !== undefined) {
        // Only a remote branch is tracked, so pulling the new branch doesn't pull a local one's
        // commits into it (see `createBranch`).
        const track = target.kind === "remote" ? "--track" : "--no-track";
        await add(track, "-b", newBranch, "--", path, branch);
      } else if (target.kind === "local") {
        await add("--", path, target.name);
      } else {
        const tracking = await trackingBranch(run, branch, target.name);
        if ("existing" in tracking) await add("--", path, tracking.existing);
        else await add("--track", "-b", tracking.local, "--", path, branch);
      }
    } catch (error) {
      throw error instanceof GitError ? error.withoutHints() : error;
    }
  });
}

/**
 * Removes the worktree at `path`, as listed: its folder is deleted, uncommitted changes and all.
 * Its branch is kept. Not the main worktree, nor the one the repository was opened as, which is
 * on show; a locked one is refused by git, saying so.
 */
export async function removeWorktree(repo: Repo, path: string): Promise<void> {
  // One write, so no other change of the worktrees can move it between finding and removing it.
  await repo.exclusive(async (run) => {
    const worktree = await findWorktree(repo.path, run, path);
    if (!worktree) {
      throw new RepositoryChangedError(
        "The worktree is gone: the worktrees changed before it could be removed.",
      );
    }
    if (worktree.main) {
      throw new WorktreeRemovalBlockedError("The main worktree can't be removed.");
    }
    if (worktree.current) {
      throw new WorktreeRemovalBlockedError(
        "This worktree is the one open here, so it can't be removed from here. Open another one, and remove it from there.",
      );
    }
    try {
      await run(["worktree", "remove", "--force", "--", worktree.path]);
    } catch (error) {
      throw error instanceof GitError ? error.withoutHints() : error;
    }
  });
}

/** The worktree at `path` among those listed through `run`, by where its folder really is. */
async function findWorktree(
  root: string,
  run: GitCommand,
  path: string,
): Promise<Worktree | undefined> {
  const [{ worktrees, paths }, wanted] = await Promise.all([listed(root, run), realPath(path)]);
  return worktrees.find((_worktree, i) => paths[i] === wanted);
}

/**
 * The worktrees listed through `run`: the main one first, then the linked ones by their paths,
 * with the one at `root`, where the repository was opened, marked `current`; and where each
 * one's folder really is, in the same order.
 */
async function listed(
  root: string,
  run: GitCommand,
): Promise<{ worktrees: Worktree[]; paths: string[] }> {
  const [main, ...linked] = parseWorktreeList(await run(WORKTREE_LIST_ARGS));
  if (!main) return { worktrees: [], paths: [] };
  // Git lists the linked ones as it finds their folders, in no set order.
  linked.sort((a, b) => a.path.localeCompare(b.path));
  const sorted = [main, ...linked];
  const [here, ...paths] = await Promise.all([root, ...sorted.map((w) => w.path)].map(realPath));
  for (const [i, worktree] of sorted.entries()) worktree.current = paths[i] === here;
  return { worktrees: sorted, paths };
}

/** Where `path` really is, symlinks resolved; `path` itself if it's gone. */
function realPath(path: string): Promise<string> {
  return realpath(path).catch(() => path);
}
