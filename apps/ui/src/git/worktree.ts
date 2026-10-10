import type { Ref, Worktree } from "@gitto/git/types";

import { branchName } from "./status";

/** The worktree the local branch `ref` (a full ref name) is checked out in, if any. */
export function worktreeOn(
  worktrees: readonly Worktree[] | undefined,
  ref: string,
): Worktree | undefined {
  return worktrees?.find((worktree) => worktree.branch === ref);
}

/**
 * The local branch switching to the remote branch `ref` (a full ref name) switches to, among
 * `refs`: the one tracking it, the current one or the one named after it if several do (as
 * `trackingBranch` picks it in the git package); `undefined` if none does, when a new one is made.
 */
export function trackingBranch(refs: readonly Ref[], ref: string): Ref | undefined {
  const name = branchName(ref);
  const tracking = refs.filter(
    (candidate) => candidate.kind === "local" && candidate.upstream === name,
  );
  return (
    tracking.find((candidate) => candidate.current) ??
    tracking.find((candidate) => name.endsWith(`/${candidate.name}`)) ??
    tracking[0]
  );
}

/**
 * The worktree checking the branch `ref` (a full ref name) out in a new one would clash with, if
 * any: the one it's checked out in, or for a remote branch, the one the local branch tracking it
 * is (see `trackingBranch`).
 */
export function checkedOutIn(
  worktrees: readonly Worktree[] | undefined,
  refs: readonly Ref[] | undefined,
  ref: string,
): Worktree | undefined {
  if (!ref.startsWith("refs/remotes/")) return worktreeOn(worktrees, ref);
  const local = trackingBranch(refs ?? [], ref);
  return local && worktreeOn(worktrees, local.fullName);
}

/**
 * Whether a worktree can be opened: not the one on show, a bare one, which has no files, nor one
 * whose folder is gone.
 */
export function canOpenWorktree(worktree: Worktree): boolean {
  return !worktree.current && !worktree.bare && worktree.prunable === null;
}

/** The path separator `path` uses: `\` on Windows, `/` elsewhere. */
function separatorOf(path: string): string {
  return path.includes("\\") && !path.includes("/") ? "\\" : "/";
}

/**
 * Where to suggest a worktree for the branch `branch` of the repository at `repository`: beside
 * it, named after both, e.g. `/home/me/gitto-feature-login` for `feature/login` of
 * `/home/me/gitto`. The branch's slashes become dashes, so it's one folder.
 */
export function suggestWorktreeFolder(repository: string, branch: string): string {
  const separator = separatorOf(repository);
  const trimmed = repository.endsWith(separator) ? repository.slice(0, -1) : repository;
  const slug = branch.replace(/[\\/:*?"<>|\s]+/g, "-").replace(/^-+|-+$/g, "") || "worktree";
  return `${trimmed}-${slug}`;
}

/**
 * The worktree in words, for a tooltip: its path, the branch it's on or the commit it's at, and
 * whether it's the one on show, locked, or its folder is gone.
 */
export function describeWorktree(worktree: Worktree): string {
  const lines = [worktree.path];
  if (worktree.bare) lines.push("Bare: no files are checked out in it");
  else if (worktree.branch) {
    lines.push(`On ${branchName(worktree.branch)}`);
  } else if (worktree.head) lines.push(`Detached at ${worktree.head.slice(0, 7)}`);
  if (worktree.current) lines.push("Open here");
  if (worktree.locked !== null) {
    lines.push(worktree.locked ? `Locked: ${worktree.locked}` : "Locked");
  }
  if (worktree.prunable !== null) lines.push("Its folder is gone");
  return lines.join("\n");
}
