import type { Conflict } from "@gitto/git/types";
import { describe, expect, it } from "vitest";

import {
  conflictKind,
  describeConflict,
  describeSide,
  keepLabel,
  operationProgress,
  operationTitle,
} from "./conflicts";

const file = { mode: "100644", oid: "a".repeat(40) };

function conflict(sides: Partial<Conflict>): Conflict {
  return {
    base: file,
    ours: file,
    theirs: file,
    text: { contents: "text", version: "v1" },
    version: "v1",
    binary: false,
    unreadable: null,
    ...sides,
  };
}

describe("conflictKind", () => {
  it("resolves a text file changed on both sides in its text", () => {
    expect(conflictKind(conflict({}))).toEqual({ kind: "text", contents: "text", version: "v1" });
    expect(conflictKind(conflict({ base: null }))).toMatchObject({ kind: "text" });
  });

  it("keeps a side whole otherwise", () => {
    expect(conflictKind(conflict({ theirs: null }))).toEqual({ kind: "sides" });
    expect(conflictKind(conflict({ text: null, binary: true }))).toEqual({ kind: "sides" });
    expect(conflictKind(conflict({ ours: { mode: "160000", oid: "b".repeat(40) } }))).toEqual({
      kind: "sides",
    });
  });

  it("has nothing to resolve once it isn't conflicted", () => {
    expect(conflictKind(conflict({ base: null, ours: null, theirs: null }))).toEqual({
      kind: "resolved",
    });
  });
});

describe("what each side did", () => {
  it("says which side deleted or added the file", () => {
    expect(describeConflict({ base: file, ours: file, theirs: file })).toBe(
      "Changed on both sides",
    );
    expect(describeConflict({ base: null, ours: file, theirs: file })).toBe("Added on both sides");
    expect(describeConflict({ base: file, ours: file, theirs: null })).toBe(
      "Deleted in theirs, changed in ours",
    );
    expect(describeConflict({ base: file, ours: null, theirs: file })).toBe(
      "Deleted in ours, changed in theirs",
    );
    expect(describeConflict({ base: null, ours: null, theirs: file })).toBe(
      "Added in theirs, not in ours",
    );
    expect(describeConflict({ base: file, ours: null, theirs: null })).toBe(
      "Deleted on both sides",
    );
  });

  it("says what a side has, and what keeping it does", () => {
    expect(describeSide(null)).toBe("Deleted");
    expect(describeSide({ mode: "120000", oid: "c" })).toBe("A symbolic link");
    expect(describeSide({ mode: "160000", oid: "1a2b3c4d5e" })).toBe("A submodule at 1a2b3c4");
    expect(keepLabel({ base: file, ours: file, theirs: null }, "ours")).toBe("Keep ours");
    expect(keepLabel({ base: file, ours: file, theirs: null }, "theirs")).toBe(
      "Delete, as theirs does",
    );
  });
});

describe("what's under way", () => {
  it("names it, and how far it's got", () => {
    expect(operationTitle({ kind: "merge", merging: "feature", into: "main" })).toBe(
      "Merging feature into main",
    );
    expect(operationTitle({ kind: "merge", merging: "1a2b3c4", into: null })).toBe(
      "Merging 1a2b3c4",
    );
    const rebase = {
      kind: "rebase",
      branch: "feature",
      onto: "origin/main",
      steps: { step: 3, total: 7 },
    } as const;
    expect(operationTitle(rebase)).toBe("Rebasing feature onto origin/main");
    expect(operationProgress(rebase)).toBe("3/7");
    const pick = {
      kind: "cherry-pick",
      commit: { sha: "1a2b3c4", subject: "Fix it" },
      remaining: 2,
    } as const;
    expect(operationTitle(pick)).toBe("Cherry-picking 1a2b3c4 Fix it");
    expect(operationProgress(pick)).toBe("2 more to go");
    expect(operationProgress({ ...pick, remaining: 0 })).toBeUndefined();
  });
});
