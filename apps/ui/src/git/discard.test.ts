import type { ChangedFile, StatusCounts } from "@gitto/git/types";
import { describe, expect, it } from "vitest";

import { canDiscard, discardAllBlocker, discardDescription, keptMessage } from "./discard";

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
    expect(canDiscard({ ...file("mod", "modified"), submodule: true })).toBe(false);
  });

  it("says what's lost, by the side it's listed on", () => {
    expect(discardDescription(file("a.txt", "modified"), "unstaged")).toBe(
      "The changes to a.txt that aren't staged are lost; its staged ones stay.",
    );
    expect(discardDescription(file("gone.txt", "deleted"), "unstaged")).toBe(
      "gone.txt is put back as it's staged, or, if it was only marked to be added, taken out of the index.",
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
    // Added with --intent-to-add, and listed as a copy too.
    expect(discardDescription(file("new.txt", "added"), "unstaged")).toBe(
      "new.txt is new, so it's deleted for good.",
    );
    expect(discardDescription({ ...file("b.txt", "copied"), origPath: "a.txt" }, "unstaged")).toBe(
      "b.txt is new, so it's deleted for good.",
    );
  });

  it("says a rename's previous path is put back, but not a copy's source, which is another file", () => {
    const renamed = { ...file("d.txt", "renamed"), origPath: "c.txt" };
    const copied = { ...file("copy.txt", "copied"), origPath: "a.txt" };
    expect(discardDescription(renamed, "staged")).toBe(
      "d.txt is deleted, and c.txt, which it was renamed from, is put back as the last commit has it. Its changes are lost.",
    );
    expect(discardDescription(copied, "staged")).toBe(
      "copy.txt is new since the last commit, so it's deleted, along with its unstaged changes.",
    );
  });
});

describe("discarding all changes", () => {
  it("says which changes it kept, the first few by name, and why", () => {
    expect(keptMessage([{ path: "mod", reason: "submodule" }])).toBe(
      "Kept mod: a submodule's changes are discarded in it.",
    );
    expect(
      keptMessage([
        { path: "a", reason: "in-the-way" },
        { path: "b", reason: "undeletable" },
        { path: "c", reason: "in-the-way" },
        { path: "d", reason: "in-the-way" },
      ]),
    ).toBe(
      "Kept a, b, c and 1 more: a deleted file isn't put back over what has taken its place; an untracked file couldn't be deleted.",
    );
  });

  it("waits for conflicts to be resolved, and needs changes", () => {
    expect(discardAllBlocker(counts({ files: 2, unstaged: 2 }), null)).toBeUndefined();
    expect(discardAllBlocker(counts({ files: 2, conflicted: 1 }), null)).toBe(
      "Resolve the conflicts before discarding all changes.",
    );
    expect(discardAllBlocker(counts({}), null)).toBe("No changes to discard.");
  });

  it("waits for an operation under way to be finished or aborted", () => {
    const merge = { kind: "merge", merging: "side", into: "main" } as const;
    expect(discardAllBlocker(counts({ files: 1, staged: 1 }), merge)).toBe(
      "Finish or abort the merge before discarding all changes.",
    );
  });
});
