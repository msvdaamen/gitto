import { describe, expect, it } from "vitest";

import { noUpstream, pullBlocker } from "./pull-blocker";

describe("pullBlocker", () => {
  it("is why a branch can't be pulled into, going by the status", () => {
    expect(pullBlocker("main", "origin/main")).toBeUndefined();
    expect(pullBlocker("main", null)).toBe(noUpstream("main"));
    expect(pullBlocker("main", "")).toBe(noUpstream("main"));
  });

  it("can't tell a detached HEAD from a rebase under way", () => {
    expect(pullBlocker(null, null)).toMatch(/detached, or a rebase/);
    expect(pullBlocker(null, "origin/main")).toMatch(/detached, or a rebase/);
  });
});
