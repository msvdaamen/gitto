// Shared with the renderer, which disables pulling for the same reasons, so this imports nothing.

/** Why a detached HEAD can't be pulled into. */
export const NO_BRANCH = "No branch is checked out to pull into.";

/** Why `branch` can't be pulled into when it has no upstream. */
export function noUpstream(branch: string): string {
  return `${branch} doesn't track a remote branch.`;
}

/** Why `branch` (`null` when HEAD is detached) can't be pulled into, if it can't. */
export function pullBlocker(branch: string | null, upstream: string | null): string | undefined {
  if (branch === null) return NO_BRANCH;
  if (!upstream) return noUpstream(branch);
  return undefined;
}
