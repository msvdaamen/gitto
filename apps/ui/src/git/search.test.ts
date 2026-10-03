import { describe, expect, it } from "vitest";

import type { CommitRow, StashRow, WipRow } from "./rows";
import { matchesSearch, searchNeedle } from "./search";

const commit: CommitRow = {
  kind: "commit",
  id: "a1b2c3d4e5",
  repositoryId: "repo",
  shortSha: "a1b2c3d",
  message: "Fix the parser",
  author: "Ada Lovelace",
  initials: "AL",
  avatarColor: "#000",
  timestamp: "Just now",
  labels: [{ kind: "branch", name: "main", local: true, remotes: ["origin"], current: true }],
};

const stash: StashRow = {
  kind: "stash",
  id: "stash:f00dfeed",
  sha: "f00dfeed",
  repositoryId: "repo",
  graph: undefined,
  message: "WIP on main: a1b2c3d Fix the parser",
  timestamp: "Just now",
};

const wip: WipRow = { kind: "wip", id: "wip", repositoryId: "repo", graph: undefined };

describe("matchesSearch", () => {
  it("matches a commit by its message, author, SHA or labels", () => {
    expect(matchesSearch(commit, searchNeedle(" PARSER "))).toBe(true);
    expect(matchesSearch(commit, searchNeedle("lovelace"))).toBe(true);
    expect(matchesSearch(commit, searchNeedle("a1b2"))).toBe(true);
    expect(matchesSearch(commit, searchNeedle("origin/main"))).toBe(true);
    expect(matchesSearch(commit, searchNeedle("develop"))).toBe(false);
  });

  it("matches a stash by its message or SHA", () => {
    expect(matchesSearch(stash, searchNeedle("wip on main"))).toBe(true);
    expect(matchesSearch(stash, searchNeedle("f00d"))).toBe(true);
    expect(matchesSearch(stash, searchNeedle("stash:"))).toBe(false);
  });

  it("matches the uncommitted changes by what their row says", () => {
    expect(matchesSearch(wip, searchNeedle("uncommitted"))).toBe(true);
    expect(matchesSearch(wip, searchNeedle("parser"))).toBe(false);
  });
});
