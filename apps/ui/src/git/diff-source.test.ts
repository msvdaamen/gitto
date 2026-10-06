import { describe, expect, it } from "vitest";

import { diffFileKey, isSameSource, shownPathIn, type FileOpener } from "./diff-source";

const opener = (shown: FileOpener["shown"]): FileOpener => ({
  open: () => {},
  prefetch: () => {},
  shown,
  beforeChange: async () => true,
});

describe("diff sources", () => {
  it("tell the same path in the staged and the unstaged changes apart", () => {
    expect(diffFileKey({ kind: "staged" }, "a.txt")).not.toBe(
      diffFileKey({ kind: "unstaged" }, "a.txt"),
    );
    expect(isSameSource({ kind: "stash", sha: "a1" }, { kind: "stash", sha: "a1" })).toBe(true);
    expect(isSameSource({ kind: "stash", sha: "a1" }, { kind: "commit", sha: "a1" })).toBe(false);
  });

  it("mark the file on show only in the list it's from", () => {
    const files = opener({ source: { kind: "staged" }, path: "a.txt" });
    expect(shownPathIn(files, { kind: "staged" })).toBe("a.txt");
    expect(shownPathIn(files, { kind: "unstaged" })).toBeUndefined();
    expect(shownPathIn(opener(undefined), { kind: "staged" })).toBeUndefined();
  });
});
