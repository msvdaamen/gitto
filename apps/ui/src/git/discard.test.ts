import type { ChangedFile, StatusCounts } from "@gitto/git/types";
import { describe, expect, it } from "vitest";

import { canDiscard, discardAllBlocker, discardDescription } from "./discard";

function file(path: string, status: ChangedFile["status"]): ChangedFile {
  return { path, status, origPath: null, additions: null, deletions: null };
}

function counts(changed: Partial<StatusCounts>): { counts: StatusCounts } {
  return { counts: { files: 0, staged: 0, unstaged: 0, conflicted: 0, ...changed } };
}

describe("discarding a file's changes", () => {
  it("is left to resolving a conflicted file, and can't delete a repository inside this one", () => {
    expect(canDiscard(file("a.txt", "modified"))).toBe(true);
    expect(canDiscard(file("new.txt", "untracked"))).toBe(true);
    expect(canDiscard(file("a.txt", "conflicted"))).toBe(false);
    expect(canDiscard(file("nested/", "untracked"))).toBe(false);
  });

  it("says what's lost, by the side it's listed on", () => {
    expect(discardDescription(file("a.txt", "modified"), "unstaged")).toBe(
      "The changes to a.txt that aren't staged are lost; its staged ones stay.",
    );
    expect(discardDescription(file("new.txt", "untracked"), "unstaged")).toBe(
      "new.txt isn't tracked, so it's deleted for good.",
    );
    expect(discardDescription(file("a.txt", "modified"), "staged")).toBe(
      "a.txt goes back to how the last commit has it: its staged and unstaged changes are lost.",
    );
    expect(discardDescription(file("new.txt", "added"), "staged")).toBe(
      "new.txt is new since the last commit, so it's deleted, along with its unstaged changes.",
    );
  });
});

describe("discarding all changes", () => {
  it("waits for conflicts to be resolved, and needs changes", () => {
    expect(discardAllBlocker(counts({ files: 2, unstaged: 2 }))).toBeUndefined();
    expect(discardAllBlocker(counts({ files: 2, conflicted: 1 }))).toBe(
      "Resolve the conflicts before discarding all changes.",
    );
    expect(discardAllBlocker(counts({}))).toBe("No changes to discard.");
  });
});
