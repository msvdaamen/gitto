import type { Stash, StatusSummary } from "@gitto/git/types";

/** Why the changes can't be stashed, if they can't. */
export function stashBlocker(status: Pick<StatusSummary, "head" | "counts">): string | undefined {
  // Git keeps a stash as commits on top of HEAD, so it needs one.
  if (status.head.kind === "unborn") return "Make the first commit before stashing changes.";
  if (status.counts.conflicted) return "Resolve the conflicts before stashing.";
  if (!status.counts.files) return "No changes to stash.";
  return undefined;
}

/** What stashing would do, e.g. "Stash 3 changed files", or why it can't. */
export function stashTitle(status: Pick<StatusSummary, "head" | "counts">): string {
  const files = status.counts.files;
  return stashBlocker(status) ?? `Stash ${files} changed ${files === 1 ? "file" : "files"}`;
}

/** Why the newest stash can't be popped, if it can't. */
export function popBlocker(
  status: Pick<StatusSummary, "counts">,
  stashes: readonly Stash[],
): string | undefined {
  if (!stashes.length) return "No stashes to pop.";
  // Git won't pop over them.
  if (status.counts.conflicted) return "Resolve the conflicts before popping a stash.";
  return undefined;
}

/** Whether a stash can be popped: the status and the stashes have loaded, and nothing blocks it. */
export function canPop(
  status: Pick<StatusSummary, "counts"> | undefined,
  stashes: readonly Stash[] | undefined,
): boolean {
  return !!status && !!stashes && !popBlocker(status, stashes);
}

/** Which stash popping would put back, or why it can't. */
export function popTitle(status: Pick<StatusSummary, "counts">, stashes: readonly Stash[]): string {
  return popBlocker(status, stashes) ?? `Pop "${stashes[0]!.message}"`;
}
