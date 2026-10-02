// Shared with the renderer, which disables pulling for the same reasons, so this imports nothing.

/** Why `branch` (`null` when HEAD is detached) can't be pulled into, if it can't. */
export function pullBlocker(branch: string | null, upstream: string | null): string | undefined {
  if (branch === null) return "Check out a branch to pull into it.";
  if (!upstream) return `${branch} doesn't track a remote branch.`;
  return undefined;
}
