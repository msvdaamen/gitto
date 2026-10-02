import type { Head, Status } from "@gitto/git/types";

/** What to call HEAD: the branch name, or "detached HEAD". */
export function headLabel(head: Head): string {
  return head.kind === "detached" ? "detached HEAD" : head.name;
}

/** The commit HEAD points at; `undefined` on a branch without commits yet. */
export function headSha(head: Head): string | undefined {
  return head.kind === "unborn" ? undefined : head.sha;
}

export function hasUncommittedChanges(status: Status | undefined): boolean {
  return !!status && status.files.length > 0;
}

/**
 * How many files have staged changes, unstaged changes and conflicts. A conflict isn't counted as
 * staged or unstaged, though git reports it on both sides.
 */
export function statusCounts(status: Status | undefined) {
  const files = status?.files ?? [];
  return {
    staged: files.filter((file) => file.staged && file.staged !== "conflicted").length,
    unstaged: files.filter((file) => file.unstaged && file.unstaged !== "conflicted").length,
    conflicted: files.filter((file) => file.staged === "conflicted").length,
  };
}

/** How HEAD compares to its upstream: "synced", or how many commits ahead and behind, e.g. "↑2 ↓1". */
export function syncLabel(status: Pick<Status, "ahead" | "behind">): string {
  const parts = [
    ...(status.ahead ? [`↑${status.ahead}`] : []),
    ...(status.behind ? [`↓${status.behind}`] : []),
  ];
  return parts.length ? parts.join(" ") : "synced";
}
