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
});
