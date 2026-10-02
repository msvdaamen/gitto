import type { Commit } from "@gitto/git/types";
import { describe, expect, it } from "vitest";

import { buildHistory, WIP_ID } from "./rows";

function commit(sha: string, parents: string[]): Commit {
  return {
    sha,
    parents,
    authorName: "Ada Lovelace",
    authorEmail: "ada@example.com",
    authoredAt: 0,
    committedAt: 0,
    refs: [],
    subject: sha,
    body: "",
  };
}

// d and c both branch off b.
const commits = [commit("d", ["b"]), commit("c", ["b"]), commit("b", ["a"]), commit("a", [])];
const input = { repositoryId: "repo", hasChanges: false, head: "d" };

describe("buildHistory", () => {
  it("puts the uncommitted changes above the commits, with a line to HEAD", () => {
    const history = buildHistory({ ...input, commits, hasChanges: true, head: "c" });
    expect(history.rows.map((row) => row.id)).toEqual([WIP_ID, "d", "c", "b", "a"]);
    expect(history.rows[0]!.graph).toMatchObject({ column: 0, bottom: [{ dashed: true }] });
    // Dashed down to `c`, past `d`.
    expect(history.rows[1]!.graph).toMatchObject({ column: 1, through: [{ dashed: true }] });
    expect(history.rows[2]!.graph).toMatchObject({ column: 0, top: [{ dashed: true }] });
  });

  it("only builds the rows of commits added to the end", () => {
    const first = buildHistory({ ...input, commits: commits.slice(0, 2) });
    const more = buildHistory({ ...input, commits }, first);
    // The rows there already are kept, the others are as if built all at once.
    expect(more.rows[0]).toBe(first.rows[0]);
    expect(more.rows[1]).toBe(first.rows[1]);
    expect(more.rows).toEqual(buildHistory({ ...input, commits }).rows);
  });

  it("builds all rows again when anything else changed", () => {
    const first = buildHistory({ ...input, commits: commits.slice(0, 2) });

    const withChanges = buildHistory({ ...input, commits, hasChanges: true }, first);
    expect(withChanges.rows).toEqual(buildHistory({ ...input, commits, hasChanges: true }).rows);
    expect(withChanges.rows[1]).not.toBe(first.rows[0]);

    // Reloaded: the same commits, as new objects.
    const reloaded = commits.map((each) => ({ ...each }));
    expect(buildHistory({ ...input, commits: reloaded }, first).rows[0]).not.toBe(first.rows[0]);
  });

  it("draws the line to HEAD once it's among the commits", () => {
    const changed = { ...input, hasChanges: true, head: "a" };
    const first = buildHistory({ ...changed, commits: commits.slice(0, 2) });
    expect(first.rows[0]!.graph).toMatchObject({ bottom: [] });

    const more = buildHistory({ ...changed, commits }, first);
    expect(more.rows).toEqual(buildHistory({ ...changed, commits }).rows);
    expect(more.rows[0]!.graph).toMatchObject({ bottom: [{ dashed: true }] });
  });
});
