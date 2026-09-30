import { describe, expect, it } from "vitest";

import { toRefLabels } from "./ref-labels";

describe("toRefLabels", () => {
  it("merges a local branch with its remote copies and puts the checked-out one first", () => {
    expect(
      toRefLabels([
        { kind: "local", name: "develop" },
        { kind: "head", name: "HEAD" },
        { kind: "local", name: "main" },
        { kind: "remote", name: "origin/main" },
        { kind: "remote", name: "upstream/main" },
        { kind: "tag", name: "v1" },
      ]),
    ).toEqual([
      { kind: "branch", name: "main", local: true, remotes: ["origin", "upstream"], current: true },
      { kind: "branch", name: "develop", local: true, remotes: [], current: false },
      { kind: "tag", name: "v1" },
    ]);
  });

  it("keeps a remote branch without a local copy under its remote's name", () => {
    expect(toRefLabels([{ kind: "remote", name: "origin/feature/login" }])).toEqual([
      {
        kind: "branch",
        name: "origin/feature/login",
        local: false,
        remotes: ["origin"],
        current: false,
      },
    ]);
  });

  it("matches a remote branch whose name has slashes to its local branch", () => {
    expect(
      toRefLabels([
        { kind: "remote", name: "origin/feature/login" },
        { kind: "local", name: "feature/login" },
      ]),
    ).toEqual([
      { kind: "branch", name: "feature/login", local: true, remotes: ["origin"], current: false },
    ]);
  });

  it("keeps the whole name of a remote ref without a remote prefix", () => {
    expect(toRefLabels([{ kind: "remote", name: "backup" }])).toEqual([
      { kind: "branch", name: "backup", local: false, remotes: [], current: false },
    ]);
  });

  it("shows a detached HEAD on its own", () => {
    expect(
      toRefLabels([
        { kind: "head", name: "HEAD" },
        { kind: "remote", name: "origin/main" },
      ]),
    ).toEqual([
      { kind: "head", name: "HEAD" },
      { kind: "branch", name: "origin/main", local: false, remotes: ["origin"], current: false },
    ]);
  });
});
