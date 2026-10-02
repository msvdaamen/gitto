import type { Head, StatusSummary } from "@gitto/git/types";

/** What to call HEAD: the branch name, or "detached HEAD". */
export function headLabel(head: Head): string {
  return head.kind === "detached" ? "detached HEAD" : head.name;
}

/** The commit HEAD points at; `undefined` on a branch without commits yet. */
export function headSha(head: Head): string | undefined {
  return head.kind === "unborn" ? undefined : head.sha;
}

/** The commit HEAD points at, which can be amended, and the upstream it's already on, if any. */
export interface LastCommit {
  sha: string;
  pushedTo: string | undefined;
}

/** The commit HEAD points at; `undefined` on a branch without commits yet. */
export function lastCommit(status: StatusSummary): LastCommit | undefined {
  const sha = headSha(status.head);
  if (!sha) return undefined;
  const pushedTo = status.upstream && status.ahead === 0 ? status.upstream : undefined;
  return { sha, pushedTo };
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
