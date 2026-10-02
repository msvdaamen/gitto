import type { StatusSummary } from "@gitto/git/types";
import { describe, expect, it } from "vitest";

import { hasUncommittedChanges, headLabel, headSha, pullTitle, syncLabel } from "./status";

const status: StatusSummary = {
  head: { kind: "branch", name: "main", sha: "abc" },
  upstream: null,
  ahead: 0,
  behind: 0,
  counts: { files: 3, staged: 1, unstaged: 2, conflicted: 1 },
};

describe("HEAD", () => {
  it("names the branch, or says it's detached", () => {
    expect(headLabel(status.head)).toBe("main");
    expect(headLabel({ kind: "detached", sha: "abc" })).toBe("detached HEAD");
  });

  it("has no commit on an unborn branch", () => {
    expect(headSha(status.head)).toBe("abc");
    expect(headSha({ kind: "unborn", name: "main" })).toBeUndefined();
  });
});

describe("hasUncommittedChanges", () => {
  it("is true when any file changed", () => {
    expect(hasUncommittedChanges(status)).toBe(true);
    const clean = { files: 0, staged: 0, unstaged: 0, conflicted: 0 };
    expect(hasUncommittedChanges({ ...status, counts: clean })).toBe(false);
    expect(hasUncommittedChanges(undefined)).toBe(false);
  });
});

describe("syncLabel", () => {
  it("says synced, or how far ahead and behind HEAD is", () => {
    expect(syncLabel({ ahead: 0, behind: 0 })).toBe("synced");
    expect(syncLabel({ ahead: 2, behind: 0 })).toBe("↑2");
    expect(syncLabel({ ahead: 2, behind: 1 })).toBe("↑2 ↓1");
  });
});

describe("pullTitle", () => {
  const tracking = { ...status, upstream: "origin/main" };

  it("says how many commits there are to pull, as of the last fetch", () => {
    expect(pullTitle(tracking)).toBe("Pull from origin/main");
    expect(pullTitle({ ...tracking, behind: 1 })).toBe("Pull 1 commit from origin/main");
    expect(pullTitle({ ...tracking, behind: 3 })).toBe("Pull 3 commits from origin/main");
  });

  it("says why there's nothing to pull", () => {
    expect(pullTitle(status)).toBe("main doesn't track a remote branch.");
    expect(pullTitle({ ...tracking, head: { kind: "detached", sha: "abc" } })).toBe(
      "No branch is checked out to pull into.",
    );
  });
});
