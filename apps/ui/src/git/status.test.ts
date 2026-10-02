import type { StatusSummary } from "@gitto/git/types";
import { describe, expect, it } from "vitest";

import { hasUncommittedChanges, headLabel, headSha, lastCommit, syncLabel } from "./status";

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

describe("lastCommit", () => {
  it("is HEAD's commit, with the upstream when that has it too", () => {
    expect(lastCommit(status)).toEqual({ sha: "abc", pushedTo: undefined });
    expect(lastCommit({ ...status, upstream: "origin/main" })).toEqual({
      sha: "abc",
      pushedTo: "origin/main",
    });
    expect(lastCommit({ ...status, upstream: "origin/main", ahead: 1 })).toEqual({
      sha: "abc",
      pushedTo: undefined,
    });
    expect(lastCommit({ ...status, head: { kind: "unborn", name: "main" } })).toBeUndefined();
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
