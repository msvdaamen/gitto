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
        [
          "HEAD -> refs/heads/main",
          "refs/remotes/origin/main",
          "refs/remotes/origin/HEAD",
          "refs/heads/origin/local",
          "tag: refs/tags/v1",
          "refs/stash",
        ].join(", "),
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
        refs: [
          { kind: "local", name: "main", current: true },
          { kind: "remote", name: "origin/main" },
          // A local branch that only looks like a remote one.
          { kind: "local", name: "origin/local" },
          { kind: "tag", name: "v1" },
        ],
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

  it("tells a detached HEAD from a checked-out branch on the same commit", () => {
    const output = ["aaa", "", "Ada", "ada@example.com", "1", "HEAD, refs/heads/main", "S", ""];

    expect(parseLog(output.join("\0"))[0]!.refs).toEqual([
      { kind: "head", name: "HEAD" },
      { kind: "local", name: "main" },
    ]);
  });

  it("returns nothing for an empty log", () => {
    expect(parseLog("")).toEqual([]);
  });
});
