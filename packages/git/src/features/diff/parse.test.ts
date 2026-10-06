import { describe, expect, it } from "vitest";

import { parseDiff } from "./parse";

describe("parseDiff", () => {
  it("combines raw statuses with numstat counts", () => {
    const output = [
      ":100644 100644 aaa bbb M",
      "src/a file.ts",
      ":100644 100644 ccc ccc R086",
      "old.ts",
      "new.ts",
      ":000000 100644 000 ddd A",
      "image.png",
      "3\t1\tsrc/a file.ts",
      "2\t0\t",
      "old.ts",
      "new.ts",
      "-\t-\timage.png",
      "",
    ].join("\0");

    expect(parseDiff(output)).toEqual([
      { path: "src/a file.ts", status: "modified", origPath: null, additions: 3, deletions: 1 },
      { path: "new.ts", status: "renamed", origPath: "old.ts", additions: 2, deletions: 0 },
      { path: "image.png", status: "added", origPath: null, additions: null, deletions: null },
    ]);
  });

  it("marks a submodule, by its mode on either side", () => {
    const output = [":160000 160000 aaa bbb M", "mod", ":000000 160000 000 ccc A", "new", ""].join(
      "\0",
    );

    expect(parseDiff(output)).toEqual([
      {
        path: "mod",
        status: "modified",
        origPath: null,
        additions: null,
        deletions: null,
        submodule: true,
      },
      {
        path: "new",
        status: "added",
        origPath: null,
        additions: null,
        deletions: null,
        submodule: true,
      },
    ]);
  });

  it("keeps a conflicted file conflicted, though it's listed as modified too", () => {
    // What `git diff --raw --numstat -z` has for a file with a conflict.
    const output = [
      ":000000 100644 000 000 U",
      "both.txt",
      ":100644 100644 aaa 000 M",
      "both.txt",
      "0\t0\tboth.txt",
      "4\t0\tboth.txt",
      "",
    ].join("\0");

    expect(parseDiff(output)).toEqual([
      { path: "both.txt", status: "conflicted", origPath: null, additions: 4, deletions: 0 },
    ]);
  });
});
