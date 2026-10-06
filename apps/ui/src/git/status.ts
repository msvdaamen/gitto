import { pullBlocker } from "@gitto/git/pull-blocker";
import type { Head, StatusSummary } from "@gitto/git/types";

/** What to call HEAD: the branch name, or "detached HEAD". */
export function headLabel(head: Head): string {
  return head.kind === "detached" ? "detached HEAD" : head.name;
}

/** The commit HEAD points at; `undefined` on a branch without commits yet. */
export function headSha(head: Head): string | undefined {
  return head.kind === "unborn" ? undefined : head.sha;
}

/** Whether HEAD is on the branch `ref`, by its full ref name, e.g. `refs/heads/main`. */
export function isCheckedOut(head: Head, ref: string): boolean {
  return head.kind !== "detached" && ref === `refs/heads/${head.name}`;
}

export function hasUncommittedChanges(status: StatusSummary | undefined): boolean {
  return !!status && status.counts.files > 0;
}

/** How HEAD compares to its upstream: "synced", or how many commits ahead and behind, e.g. "↑2 ↓1". */
export function syncLabel(status: Pick<StatusSummary, "ahead" | "behind">): string {
  const parts = [
    ...(status.ahead ? [`↑${status.ahead}`] : []),
    ...(status.behind ? [`↓${status.behind}`] : []),
  ];
  return parts.length ? parts.join(" ") : "synced";
}

/** Why HEAD can't be pulled into, if it can't: it isn't on a branch, or the branch has no upstream. */
export function headPullBlocker(
  status: Pick<StatusSummary, "head" | "upstream">,
): string | undefined {
  return pullBlocker(status.head.kind === "detached" ? null : status.head.name, status.upstream);
}

/** What pulling would do, e.g. "Pull 2 commits from origin/main", or why it can't. */
export function pullTitle(status: Pick<StatusSummary, "head" | "upstream" | "behind">): string {
  const blocker = headPullBlocker(status);
  if (blocker) return blocker;
  // `behind` is as of the last fetch; the pull fetches first, so there may be more.
  if (!status.behind) return `Pull from ${status.upstream}`;
  return `Pull ${status.behind} ${status.behind === 1 ? "commit" : "commits"} from ${status.upstream}`;
}

/** What a new branch is made from: the branch's name, or "this commit" with HEAD detached. */
export function branchSource(head: Head): string {
  return head.kind === "detached" ? "this commit" : head.name;
}
