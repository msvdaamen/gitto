import { describe, expect, it } from "vitest";

import { parseStatus } from "./parse";

const output = [
  "# branch.oid 1111111111111111111111111111111111111111",
  "# branch.head main",
  "# branch.upstream origin/main",
  "# branch.ab +2 -1",
  "1 .M N... 100644 100644 100644 aaa aaa src/a file.ts",
  "1 A. N... 000000 100644 100644 000 bbb added.ts",
  "2 R. N... 100644 100644 100644 ccc ccc R100 new name.ts",
  "old name.ts",
  "u UU N... 100644 100644 100644 100644 ddd eee fff conflict.ts",
  "? untracked file.txt",
  "",
].join("\0");

describe("parseStatus", () => {
  it("reads the branch headers", () => {
    expect(parseStatus(output)).toMatchObject({
      head: { kind: "branch", name: "main", sha: "1111111111111111111111111111111111111111" },
      upstream: "origin/main",
      ahead: 2,
      behind: 1,
    });
  });

  it("has no upstream when it's gone", () => {
    const gone = "# branch.oid 1111\0# branch.head main\0# branch.upstream origin/main\0";
    expect(parseStatus(gone)).toMatchObject({ upstream: null, ahead: 0, behind: 0 });
  });

  it("reads changed, renamed, conflicted and untracked files", () => {
    expect(parseStatus(output).files).toEqual([
      { path: "src/a file.ts", origPath: null, staged: null, unstaged: "modified" },
      { path: "added.ts", origPath: null, staged: "added", unstaged: null },
      { path: "new name.ts", origPath: "old name.ts", staged: "renamed", unstaged: null },
      { path: "conflict.ts", origPath: null, staged: "conflicted", unstaged: "conflicted" },
      { path: "untracked file.txt", origPath: null, staged: null, unstaged: "untracked" },
    ]);
  });

  it("handles a detached HEAD", () => {
    expect(parseStatus("# branch.oid 2222\0# branch.head (detached)\0")).toEqual({
      head: { kind: "detached", sha: "2222" },
      upstream: null,
      ahead: 0,
      behind: 0,
      files: [],
    });
  });

  it("handles a branch without commits", () => {
    expect(parseStatus("# branch.oid (initial)\0# branch.head main\0").head).toEqual({
      kind: "unborn",
      name: "main",
    });
  });
});
