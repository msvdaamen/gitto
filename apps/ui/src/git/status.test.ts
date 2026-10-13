import type { StatusSummary } from "@gitto/git/types";
import { describe, expect, it } from "vitest";

import { pullTitle, syncLabel } from "./status";

const status: StatusSummary = {
  head: { kind: "branch", name: "main", sha: "abc" },
  upstream: null,
  ahead: 0,
  behind: 0,
  counts: { files: 3, staged: 1, unstaged: 2, conflicted: 1 },
};

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
      "Not on a branch: HEAD is detached, or a rebase is under way.",
    );
  });
});
