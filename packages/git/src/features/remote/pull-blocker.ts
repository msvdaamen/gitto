// Shared with the renderer, which disables pulling for the same reasons, so this imports nothing.

/** Why a detached HEAD can't be pulled into, when no rebase is under way. */
export const NO_BRANCH = "HEAD is detached: check out a branch to pull into it.";

/** Why HEAD can't be pulled into while a rebase is stopped, which detaches it. */
export const REBASING = "A rebase is under way. Continue or abort it, then pull.";

/**
 * Why a detached HEAD can't be pulled into, when it's not known whether a rebase is under way:
 * the status, which the UI goes by, doesn't say.
 */
export const DETACHED = "Not on a branch: HEAD is detached, or a rebase is under way.";

/** Why `branch` can't be pulled into when it has no upstream. */
export function noUpstream(branch: string): string {
  return `${branch} doesn't track a remote branch.`;
}

/**
 * Why `branch` (`null` when HEAD is detached) can't be pulled into, if it can't, going by what
 * the status says; it can't tell a detached HEAD from a rebase under way.
 */
export function pullBlocker(branch: string | null, upstream: string | null): string | undefined {
  if (branch === null) return DETACHED;
  if (!upstream) return noUpstream(branch);
  return undefined;
}
