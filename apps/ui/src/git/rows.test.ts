import type { Commit, Stash } from "@gitto/git/types";
import { describe, expect, it } from "vitest";

import { historyGraph, stashRowId, stashSha, toHistoryRows, withStashes } from "./rows";

function commit(sha: string, parents: string[], committedAt: number): Commit {
  return {
    sha,
    parents,
    authorName: "Ada Lovelace",
    authorEmail: "ada@example.com",
    authoredAt: committedAt,
    committedAt,
    refs: [],
    subject: sha,
    body: "",
  };
}

function stash(sha: string, base: string, createdAt: number): Stash {
  return { sha, base, message: `WIP ${sha}`, createdAt };
}

/** The SHAs of `withStashes`'s entries, in order. */
function order(log: Commit[], stashes: Stash[]): string[] {
  return withStashes(log, stashes).map((entry) =>
    entry.kind === "commit" ? entry.commit.sha : entry.stash.sha,
  );
}

const log = [commit("c", ["b"], 3000), commit("b", ["a"], 2000), commit("a", [], 1000)];

describe("withStashes", () => {
  it("lists each stash just above the commit it was made on, whenever it was made", () => {
    // Made after `c` was committed, but on `b`: no line runs past `c`'s tip.
    expect(order(log, [stash("s2", "b", 9000), stash("s1", "a", 1500)])).toEqual([
      "c",
      "s2",
      "b",
      "s1",
      "a",
    ]);
    // A clock that was behind, or a commit amended to a later date.
    expect(order(log, [stash("s", "c", 500)])).toEqual(["s", "c", "b", "a"]);
  });

  it("lists stashes on the same commit newest first", () => {
    // Listed out of date order, as when the clock was turned back between them.
    expect(order(log, [stash("older", "b", 2100), stash("newer", "b", 2500)])).toEqual([
      "c",
      "newer",
      "older",
      "b",
      "a",
    ]);
  });

  it("goes by the date for a stash made on a commit that isn't in the log", () => {
    expect(order(log, [stash("s", "gone", 2500)])).toEqual(["c", "s", "b", "a"]);
    // Made in the same second as a commit: above it.
    expect(order(log, [stash("s", "gone", 2000)])).toEqual(["c", "s", "b", "a"]);
    // Older than every commit in the log: further back than it goes.
    expect(order(log, [stash("s", "gone", 10)])).toEqual(["c", "b", "a"]);
  });
});

describe("historyGraph", () => {
  it("draws a dashed line from each stash to the commit it was made on, if it's listed", () => {
    const entries = withStashes(log, [stash("s", "b", 2500), stash("t", "gone", 2600)]);
    const [, t, s, b] = historyGraph(entries, false, "c");
    // In a lane of its own, next to the one from `c`.
    expect(s).toMatchObject({ column: 1, bottom: [{ from: 1, to: 1, dashed: true }] });
    expect(t).toMatchObject({ bottom: [] });
    expect(b!.top).toEqual([
      { from: 0, to: 0 },
      { from: 1, to: 0, dashed: true },
    ]);
  });

  it("keeps the branches in their lanes when a stash was made on an older commit", () => {
    // Stashed on `b`, then `c` was pulled: `c`'s tip still starts the first lane.
    const graph = historyGraph(withStashes(log, [stash("s", "b", 9000)]), false, "c");
    expect(graph.map((row) => row.column)).toEqual([0, 1, 0, 0]);
    expect(Math.max(...graph.map((row) => row.width))).toBe(2);
  });

  it("gives the stashes rows of their own, below the uncommitted changes", () => {
    const entries = withStashes(log, [stash("s", "b", 2500)]);
    const rows = toHistoryRows("repo", entries, true, historyGraph(entries, true, "c"));
    expect(rows.map((row) => [row.kind, row.id])).toEqual([
      ["wip", "wip"],
      ["commit", "c"],
      ["stash", stashRowId("s")],
      ["commit", "b"],
      ["commit", "a"],
    ]);
    expect(rows[2]).toMatchObject({ sha: "s", message: "WIP s", graph: { column: 1 } });
  });
});

describe("stash row ids", () => {
  it("are told apart from commits' SHAs", () => {
    expect(stashSha(stashRowId("abc"))).toBe("abc");
    expect(stashSha("abc")).toBeUndefined();
  });
});
