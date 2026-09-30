import type { Commit as GitCommit, Status } from "@gitto/git/types";
import { describe, expect, it } from "vitest";

import { reuseRows, toCommitRows } from "./rows";

const log: GitCommit[] = [
  {
    sha: "b1",
    parents: ["a1"],
    authorName: "Ada Lovelace",
    authorEmail: "ada@example.com",
    authoredAt: 0,
    refs: [],
    subject: "Second",
    body: "",
  },
  {
    sha: "a1",
    parents: [],
    authorName: "Ada Lovelace",
    authorEmail: "ada@example.com",
    authoredAt: 0,
    refs: [],
    subject: "First",
    body: "",
  },
];

const status: Status = {
  head: { kind: "branch", name: "main", sha: "b1" },
  upstream: null,
  ahead: 0,
  behind: 0,
  files: [{ path: "wip.txt", origPath: null, staged: null, unstaged: "modified" }],
};

describe("reuseRows", () => {
  it("keeps the objects of rows that didn't change", () => {
    const previous = toCommitRows("repo", log, status);
    const next = reuseRows(previous, toCommitRows("repo", log, status));

    expect(next).toHaveLength(3);
    next.forEach((row, index) => expect(row).toBe(previous[index]));
  });

  it("uses the new object for a changed row", () => {
    const previous = toCommitRows("repo", log, status);
    const changed = [{ ...log[0]!, subject: "Second, reworded" }, log[1]!];
    const next = reuseRows(previous, toCommitRows("repo", changed, status));

    expect(next[1]).not.toBe(previous[1]);
    expect(next[1]!.message).toBe("Second, reworded");
    expect(next[2]).toBe(previous[2]);
  });
});
