import type { Status } from "@gitto/git/types";
import { describe, expect, it } from "vitest";

import { hasUncommittedChanges, headLabel, headSha, statusCounts, syncLabel } from "./status";

const status: Status = {
  head: { kind: "branch", name: "main", sha: "abc" },
  upstream: null,
  ahead: 0,
  behind: 0,
  files: [
    { path: "a", origPath: null, staged: "modified", unstaged: "modified" },
    { path: "b", origPath: null, staged: null, unstaged: "untracked" },
    { path: "c", origPath: null, staged: "conflicted", unstaged: "conflicted" },
  ],
};

describe("statusCounts", () => {
  it("counts conflicts apart from staged and unstaged changes", () => {
    expect(statusCounts(status)).toEqual({ staged: 1, unstaged: 2, conflicted: 1 });
  });

  it("counts nothing before the status is loaded", () => {
    expect(statusCounts(undefined)).toEqual({ staged: 0, unstaged: 0, conflicted: 0 });
  });
});

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
    expect(hasUncommittedChanges({ ...status, files: [] })).toBe(false);
  });
});

describe("syncLabel", () => {
  it("says synced, or how far ahead and behind HEAD is", () => {
    expect(syncLabel({ ahead: 0, behind: 0 })).toBe("synced");
    expect(syncLabel({ ahead: 2, behind: 0 })).toBe("↑2");
    expect(syncLabel({ ahead: 2, behind: 1 })).toBe("↑2 ↓1");
  });
});
