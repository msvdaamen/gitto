import type { Worktree } from "@gitto/git/types";
import { describe, expect, it } from "vitest";

import { checkouts, describeRefLabel, refLabelMatches, toRefLabels } from "./ref-labels";

function worktree(
  name: string,
  branch: string | null,
  head: string | null = "e5",
  extra: Partial<Worktree> = {},
): Worktree {
  return {
    path: `/w/${name}`,
    name,
    head,
    branch,
    main: false,
    bare: false,
    current: false,
    locked: null,
    prunable: null,
    ...extra,
  };
}

/** The other worktrees: one on `feature`, one detached at `e5`, besides the current one on `main`. */
const OTHERS = checkouts([
  worktree("repo", "refs/heads/main", "e5", { main: true, current: true }),
  worktree("repo-feature", "refs/heads/feature"),
  worktree("repo-check", null),
  worktree("repo-old", null, "a1"),
  worktree("bare.git", null, null, { bare: true }),
]);

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

describe("toRefLabels with other worktrees", () => {
  it("says which worktree a branch is checked out in, and names the ones detached at the commit", () => {
    expect(
      toRefLabels(
        [
          { kind: "local", name: "main", fullName: "refs/heads/main", current: true },
          { kind: "local", name: "feature", fullName: "refs/heads/feature" },
          { kind: "remote", name: "origin/feature", fullName: "refs/remotes/origin/feature" },
        ],
        "e5",
        OTHERS,
      ),
    ).toEqual([
      expect.objectContaining({ kind: "branch", name: "main", current: true }),
      expect.objectContaining({
        kind: "branch",
        name: "feature",
        worktree: { name: "repo-feature", path: "/w/repo-feature" },
      }),
      { kind: "worktree", name: "repo-check", path: "/w/repo-check" },
    ]);
    expect(toRefLabels([], "a1", OTHERS)).toEqual([
      { kind: "worktree", name: "repo-old", path: "/w/repo-old" },
    ]);
    expect(toRefLabels([], "b2", OTHERS)).toEqual([]);
  });

  it("leaves out the worktree on show, and a bare one", () => {
    expect(OTHERS.branches.has("refs/heads/main")).toBe(false);
    expect([...OTHERS.detached.values()].flat().map((at) => at.name)).toEqual([
      "repo-check",
      "repo-old",
    ]);
  });

  it("matches and describes them", () => {
    const [, feature, check] = toRefLabels(
      [
        { kind: "local", name: "main", fullName: "refs/heads/main", current: true },
        { kind: "local", name: "feature", fullName: "refs/heads/feature" },
      ],
      "e5",
      OTHERS,
    );
    expect(refLabelMatches(check!, "repo-ch")).toBe(true);
    expect(refLabelMatches(feature!, "repo-feat")).toBe(true);
    expect(refLabelMatches(feature!, "head")).toBe(false);
    expect(describeRefLabel(check!)).toBe("repo-check (worktree, detached)");
    expect(describeRefLabel(feature!)).toBe("feature (local; checked out in repo-feature)");
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
