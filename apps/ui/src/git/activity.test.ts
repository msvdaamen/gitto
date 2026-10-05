import type { Activity } from "@gitto/git/types";
import { describe, expect, it } from "vitest";

import { describeActivity } from "./activity";

const activity = (entry: Partial<Activity>): Activity => ({
  at: 0,
  kind: "other",
  target: null,
  subject: "Fix it",
  message: "",
  ...entry,
});

describe("describeActivity", () => {
  it("shows a commit by its subject", () => {
    expect(describeActivity(activity({ kind: "commit" }))).toEqual({
      title: "Fix it",
      action: "Committed",
    });
    expect(describeActivity(activity({ kind: "amend" }))).toEqual({
      title: "Fix it",
      action: "Amended",
    });
  });

  it("names what was switched, merged or reset to, with a commit's SHA shortened", () => {
    expect(describeActivity(activity({ kind: "checkout", target: "feature" }))).toEqual({
      title: "Switched to feature",
    });
    const sha = "1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b";
    expect(describeActivity(activity({ kind: "merge", target: sha }))).toEqual({
      title: "Merged 1a2b3c4",
    });
    expect(describeActivity(activity({ kind: "reset", target: "HEAD~1" }))).toEqual({
      title: "Reset to HEAD~1",
    });
    expect(describeActivity(activity({ kind: "pull" }))).toEqual({ title: "Pulled" });
  });

  it("shows what it doesn't know in git's words", () => {
    expect(
      describeActivity(activity({ message: "Branch: renamed refs/heads/a to refs/heads/b" })),
    ).toEqual({ title: "Branch: renamed a to b" });
  });
});
