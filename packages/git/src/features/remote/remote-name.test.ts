import { describe, expect, it } from "vitest";

import { remoteOf } from "./remote-name";

describe("the remote a remote branch is on", () => {
  it("is the longest remote its name starts with", () => {
    expect(remoteOf(["origin", "team", "team/origin"], "team/origin/main")).toBe("team/origin");
    expect(remoteOf(["origin"], "origin/feature/x")).toBe("origin");
  });

  it("is none for a branch of a remote that's gone", () => {
    expect(remoteOf(["origin"], "old/main")).toBe(undefined);
    expect(remoteOf(["orig"], "origin/main")).toBe(undefined);
  });
});
