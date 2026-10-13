import type { PushTarget } from "@gitto/git/types";

/** What pushing would do, e.g. "Push 2 commits to origin/main", or why it can't. */
export function pushTitle(target: PushTarget): string {
  if (target.kind === "blocked") return target.reason;
  if (target.isNew) return `Push to ${target.name}, a new remote branch`;
  if (!target.ahead) return `Push to ${target.name}`;
  return `Push ${target.ahead} ${target.ahead === 1 ? "commit" : "commits"} to ${target.name}`;
}
