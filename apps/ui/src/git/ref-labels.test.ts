import { describe, expect, it } from "vitest";

import { refLabelMatches, toRefLabels } from "./ref-labels";

describe("toRefLabels", () => {
  it("merges a local branch with its remote copies and puts the checked-out one first", () => {
    expect(
      toRefLabels([
        { kind: "local", name: "develop" },
        { kind: "local", name: "main", current: true },
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

  it("shows a detached HEAD on its own, even where a branch points too", () => {
    expect(
      toRefLabels([
        { kind: "head", name: "HEAD" },
        { kind: "local", name: "main" },
      ]),
    ).toEqual([
      { kind: "head", name: "HEAD" },
      { kind: "branch", name: "main", local: true, remotes: [], current: false },
    ]);
  });

  it("keeps a local branch named like a remote one apart from that remote branch", () => {
    expect(
      toRefLabels([
        { kind: "local", name: "origin/main", current: true },
        { kind: "remote", name: "origin/main" },
      ]),
    ).toEqual([
      { kind: "branch", name: "origin/main", local: true, remotes: [], current: true },
      { kind: "branch", name: "origin/main", local: false, remotes: ["origin"], current: false },
    ]);
  });
});

describe("refLabelMatches", () => {
  const [main, remoteOnly, tag] = toRefLabels([
    { kind: "local", name: "main" },
    { kind: "remote", name: "upstream/main" },
    { kind: "remote", name: "origin/feature" },
    { kind: "tag", name: "v1" },
  ]);

  it("matches a branch by its name or a remote copy's full name", () => {
    expect(refLabelMatches(main!, "main")).toBe(true);
    expect(refLabelMatches(main!, "upstream")).toBe(true);
    expect(refLabelMatches(main!, "origin")).toBe(false);
    expect(refLabelMatches(remoteOnly!, "origin/feat")).toBe(true);
  });

  it("matches a tag with its `tag:` prefix, case-insensitively", () => {
    expect(refLabelMatches(tag!, "tag: v1")).toBe(true);
    expect(
      refLabelMatches(
        { kind: "branch", name: "Main", local: true, remotes: [], current: false },
        "main",
      ),
    ).toBe(true);
  });
});
