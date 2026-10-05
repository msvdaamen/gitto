import { describe, expect, it } from "vitest";

import { describeRefLabel, refLabelMatches, toRefLabels } from "./ref-labels";

describe("toRefLabels", () => {
  it("merges a local branch with its remote copies and puts the checked-out one first", () => {
    expect(
      toRefLabels([
        { kind: "local", name: "develop", fullName: "refs/heads/develop" },
        { kind: "local", name: "main", fullName: "refs/heads/main", current: true },
        { kind: "remote", name: "origin/main", fullName: "refs/remotes/origin/main" },
        { kind: "remote", name: "upstream/main", fullName: "refs/remotes/upstream/main" },
        { kind: "tag", name: "v1", fullName: "refs/tags/v1" },
      ]),
    ).toEqual([
      {
        kind: "branch",
        name: "main",
        ref: "refs/heads/main",
        local: true,
        remotes: ["origin", "upstream"],
        current: true,
      },
      {
        kind: "branch",
        name: "develop",
        ref: "refs/heads/develop",
        local: true,
        remotes: [],
        current: false,
      },
      { kind: "tag", name: "v1" },
    ]);
  });

  it("keeps a remote branch without a local copy under its remote's name", () => {
    expect(
      toRefLabels([
        {
          kind: "remote",
          name: "origin/feature/login",
          fullName: "refs/remotes/origin/feature/login",
        },
      ]),
    ).toEqual([
      {
        kind: "branch",
        name: "origin/feature/login",
        ref: "refs/remotes/origin/feature/login",
        local: false,
        remotes: ["origin"],
        current: false,
      },
    ]);
  });

  it("matches a remote branch whose name has slashes to its local branch", () => {
    expect(
      toRefLabels([
        {
          kind: "remote",
          name: "origin/feature/login",
          fullName: "refs/remotes/origin/feature/login",
        },
        { kind: "local", name: "feature/login", fullName: "refs/heads/feature/login" },
      ]),
    ).toEqual([
      {
        kind: "branch",
        name: "feature/login",
        ref: "refs/heads/feature/login",
        local: true,
        remotes: ["origin"],
        current: false,
      },
    ]);
  });

  it("keeps the whole name of a remote ref without a remote prefix", () => {
    expect(
      toRefLabels([{ kind: "remote", name: "backup", fullName: "refs/remotes/backup" }]),
    ).toEqual([
      {
        kind: "branch",
        name: "backup",
        ref: "refs/remotes/backup",
        local: false,
        remotes: [],
        current: false,
      },
    ]);
  });

  it("shows a detached HEAD on its own, even where a branch points too", () => {
    expect(
      toRefLabels([
        { kind: "head", name: "HEAD", fullName: "HEAD" },
        { kind: "local", name: "main", fullName: "refs/heads/main" },
      ]),
    ).toEqual([
      { kind: "head", name: "HEAD" },
      {
        kind: "branch",
        name: "main",
        ref: "refs/heads/main",
        local: true,
        remotes: [],
        current: false,
      },
    ]);
  });

  it("keeps a local branch named like a remote one apart from that remote branch", () => {
    expect(
      toRefLabels([
        { kind: "local", name: "origin/main", fullName: "refs/heads/origin/main", current: true },
        { kind: "remote", name: "origin/main", fullName: "refs/remotes/origin/main" },
      ]),
    ).toEqual([
      {
        kind: "branch",
        name: "origin/main",
        ref: "refs/heads/origin/main",
        local: true,
        remotes: [],
        current: true,
      },
      {
        kind: "branch",
        name: "origin/main",
        ref: "refs/remotes/origin/main",
        local: false,
        remotes: ["origin"],
        current: false,
      },
    ]);
  });
});

describe("refLabelMatches", () => {
  const [main, remoteOnly, tag] = toRefLabels([
    { kind: "local", name: "main", fullName: "refs/heads/main" },
    { kind: "remote", name: "upstream/main", fullName: "refs/remotes/upstream/main" },
    { kind: "remote", name: "origin/feature", fullName: "refs/remotes/origin/feature" },
    { kind: "tag", name: "v1", fullName: "refs/tags/v1" },
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
        {
          kind: "branch",
          name: "Main",
          ref: "refs/heads/Main",
          local: true,
          remotes: [],
          current: false,
        },
        "main",
      ),
    ).toBe(true);
  });
});

describe("refLabelMatches for the checked-out branch", () => {
  it("matches `HEAD`, like the branch's decoration in git", () => {
    const [main] = toRefLabels([
      { kind: "local", name: "main", fullName: "refs/heads/main", current: true },
    ]);
    expect(refLabelMatches(main!, "head")).toBe(true);
  });
});

describe("describeRefLabel", () => {
  it("spells out what the pill's icons show", () => {
    expect(
      toRefLabels([
        { kind: "local", name: "main", fullName: "refs/heads/main", current: true },
        { kind: "remote", name: "origin/main", fullName: "refs/remotes/origin/main" },
        { kind: "remote", name: "origin/feature", fullName: "refs/remotes/origin/feature" },
        { kind: "tag", name: "v1", fullName: "refs/tags/v1" },
        { kind: "head", name: "HEAD", fullName: "HEAD" },
      ]).map(describeRefLabel),
    ).toEqual([
      "HEAD (detached)",
      "main (checked out; local, origin)",
      "origin/feature (remote)",
      "v1 (tag)",
    ]);
  });
});
