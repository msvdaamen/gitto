import { describe, expect, it } from "vitest";

import { parseLog } from "./parse";

describe("parseLog", () => {
  it("reads NUL-separated commits", () => {
    // sha, parents, author name, author email, author date, decorations, subject, body
    const commits = [
      [
        "bbb",
        "aaa ccc",
        "Ada Lovelace",
        "ada@example.com",
        "1700000000",
        "HEAD -> main, origin/main, tag: v1",
        "Merge branch 'feature'",
        "Body line\n\n",
      ],
      ["aaa", "", "Grace Hopper", "grace@example.com", "1600000000", "", "Initial", ""],
    ];
    const output = commits.flat().join("\0");

    expect(parseLog(output)).toEqual([
      {
        sha: "bbb",
        parents: ["aaa", "ccc"],
        authorName: "Ada Lovelace",
        authorEmail: "ada@example.com",
        authoredAt: 1_700_000_000_000,
        refs: ["HEAD", "main", "origin/main", "tag: v1"],
        subject: "Merge branch 'feature'",
        body: "Body line",
      },
      {
        sha: "aaa",
        parents: [],
        authorName: "Grace Hopper",
        authorEmail: "grace@example.com",
        authoredAt: 1_600_000_000_000,
        refs: [],
        subject: "Initial",
        body: "",
      },
    ]);
  });

  it("returns nothing for an empty log", () => {
    expect(parseLog("")).toEqual([]);
  });
});
