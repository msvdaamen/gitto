import type { Ref, Worktree } from "@gitto/git/types";
import { describe, expect, it } from "vitest";

import {
  canOpenWorktree,
  checkedOutIn,
  describeWorktree,
  suggestWorktreeFolder,
  trackingBranch,
  worktreeOn,
} from "./worktree";

function ref(name: string, upstream: string | null, current = false): Ref {
  return {
    name,
    fullName: `refs/heads/${name}`,
    kind: "local",
    sha: "abc",
    current,
    upstream,
    ahead: 0,
    behind: 0,
  };
}

function worktree(path: string, branch: string | null, extra: Partial<Worktree> = {}): Worktree {
  return {
    path,
    name: path.slice(path.lastIndexOf("/") + 1),
    head: "0123456789abcdef",
    branch,
    main: false,
    bare: false,
    current: false,
    locked: null,
    prunable: null,
    ...extra,
  };
}

describe("worktreeOn", () => {
  it("finds the worktree a branch is checked out in, by its full ref name", () => {
    const worktrees = [worktree("/repo", "refs/heads/main"), worktree("/repo-f", "refs/heads/f")];
    expect(worktreeOn(worktrees, "refs/heads/f")?.path).toBe("/repo-f");
    expect(worktreeOn(worktrees, "refs/heads/gone")).toBeUndefined();
    expect(worktreeOn(undefined, "refs/heads/main")).toBeUndefined();
  });
});

describe("trackingBranch", () => {
  it("picks the current branch tracking the remote one, else the one named after it, else any", () => {
    const named = ref("feature", "origin/feature");
    const other = ref("my-feature", "origin/feature");
    const current = ref("wip", "origin/feature", true);
    expect(trackingBranch([other, named, current], "refs/remotes/origin/feature")).toBe(current);
    expect(trackingBranch([other, named], "refs/remotes/origin/feature")).toBe(named);
    expect(trackingBranch([other], "refs/remotes/origin/feature")).toBe(other);
    expect(trackingBranch([ref("main", "origin/main")], "refs/remotes/origin/feature")).toBe(
      undefined,
    );
  });
});

describe("checkedOutIn", () => {
  const worktrees = [
    worktree("/repo", "refs/heads/main", { current: true }),
    worktree("/repo-f", "refs/heads/feature"),
  ];
  const refs = [ref("main", "origin/main", true), ref("feature", "origin/feature")];

  it("is the worktree a local branch is checked out in", () => {
    expect(checkedOutIn(worktrees, refs, "refs/heads/feature")?.path).toBe("/repo-f");
    expect(checkedOutIn(worktrees, refs, "refs/heads/other")).toBeUndefined();
  });

  it("is the worktree the branch tracking a remote one is checked out in", () => {
    expect(checkedOutIn(worktrees, refs, "refs/remotes/origin/main")?.path).toBe("/repo");
    expect(checkedOutIn(worktrees, refs, "refs/remotes/origin/feature")?.path).toBe("/repo-f");
    expect(checkedOutIn(worktrees, refs, "refs/remotes/origin/other")).toBeUndefined();
    expect(checkedOutIn(worktrees, undefined, "refs/remotes/origin/main")).toBeUndefined();
  });
});

describe("canOpenWorktree", () => {
  it("is false for the one on show, a bare one, and one whose folder is gone", () => {
    expect(canOpenWorktree(worktree("/repo-f", "refs/heads/f"))).toBe(true);
    expect(canOpenWorktree(worktree("/repo", "refs/heads/main", { current: true }))).toBe(false);
    expect(canOpenWorktree(worktree("/bare.git", null, { bare: true, head: null }))).toBe(false);
    expect(
      canOpenWorktree(worktree("/gone", null, { prunable: "gitdir file points elsewhere" })),
    ).toBe(false);
  });
});

describe("suggestWorktreeFolder", () => {
  it("puts the worktree beside the repository, named after it and the branch", () => {
    expect(suggestWorktreeFolder("/home/me/gitto", "feature")).toBe("/home/me/gitto-feature");
    expect(suggestWorktreeFolder("/home/me/gitto/", "feature/login")).toBe(
      "/home/me/gitto-feature-login",
    );
    expect(suggestWorktreeFolder("C:\\Users\\me\\gitto", "fix\\it")).toBe(
      "C:\\Users\\me\\gitto-fix-it",
    );
  });

  it("drops what a folder name can't have, and never ends up without a name", () => {
    expect(suggestWorktreeFolder("/r", 'a: "b"?')).toBe("/r-a-b");
    expect(suggestWorktreeFolder("/r", "///")).toBe("/r-worktree");
  });
});

describe("describeWorktree", () => {
  it("says where it is, what it's on, and what state it's in", () => {
    expect(describeWorktree(worktree("/repo-f", "refs/heads/f", { current: true }))).toBe(
      "/repo-f\nOn f\nOpen here",
    );
    expect(
      describeWorktree(
        worktree("/repo-c", null, { locked: "busy", prunable: "gitdir file points elsewhere" }),
      ),
    ).toBe("/repo-c\nDetached at 0123456\nLocked: busy\nIts folder is gone");
    expect(describeWorktree(worktree("/repo-l", "refs/heads/l", { locked: "" }))).toBe(
      "/repo-l\nOn l\nLocked",
    );
    expect(describeWorktree(worktree("/bare.git", null, { bare: true, head: null }))).toBe(
      "/bare.git\nBare: no files are checked out in it",
    );
  });
});
