import type { ChangedFile } from "@gitto/git/types";
import { describe, expect, it } from "vitest";

import {
  hasPatch,
  isKnownBinary,
  isNestedRepository,
  stagingPaths,
  unchangedReason,
} from "./changes";

function file(
  path: string,
  status: ChangedFile["status"] = "modified",
  additions: number | null = 1,
  deletions: number | null = 0,
  origPath: string | null = null,
): ChangedFile {
  return { path, status, origPath, additions, deletions };
}

describe("stagingPaths", () => {
  it("stages a renamed file by both its paths", () => {
    expect(stagingPaths([file("a"), file("b", "renamed", 1, 0, "old")])).toEqual(["a", "b", "old"]);
  });
});

describe("isKnownBinary", () => {
  it("is true for a file without counts in a list that counted", () => {
    expect(isKnownBinary(file("a", "modified", null, null), false)).toBe(true);
    expect(isKnownBinary(file("a", "modified", 1, 0), false)).toBe(false);
  });

  it("can't tell in a list that didn't count, nor for an untracked file", () => {
    expect(isKnownBinary(file("a", "modified", null, null), true)).toBe(false);
    expect(isKnownBinary(file("a", "untracked", null, null), false)).toBe(false);
  });
});

describe("isNestedRepository", () => {
  it("is an untracked folder", () => {
    expect(isNestedRepository(file("sub/", "untracked", null, null))).toBe(true);
    expect(isNestedRepository(file("sub/a.txt", "untracked", null, null))).toBe(false);
    expect(isNestedRepository(file("sub/", "modified"))).toBe(false);
  });
});

describe("hasPatch", () => {
  it("is true for a file with changed lines, or whose lines weren't counted", () => {
    expect(hasPatch(file("a"), false)).toBe(true);
    expect(hasPatch(file("a", "untracked", null, null), false)).toBe(true);
    expect(hasPatch(file("a", "modified", null, null), true)).toBe(true);
  });

  it("is false for a conflicted, binary or unchanged file, or a nested repository", () => {
    expect(hasPatch(file("a", "conflicted"), false)).toBe(false);
    expect(hasPatch(file("a", "modified", null, null), false)).toBe(false);
    expect(hasPatch(file("a", "modified", 0, 0), false)).toBe(false);
    expect(hasPatch(file("sub/", "untracked", null, null), false)).toBe(false);
  });
});

describe("unchangedReason", () => {
  it("says why there are no changed lines", () => {
    expect(unchangedReason(file("b", "renamed", 0, 0, "a"))).toBe(
      "Renamed from a, with the same contents.",
    );
    expect(unchangedReason(file("a", "added", 0, 0))).toBe("An empty file was added.");
    expect(unchangedReason(file("a", "untracked", 0, 0))).toBe("An empty file was added.");
    expect(unchangedReason(file("a", "deleted", 0, 0))).toBe("An empty file was deleted.");
    expect(unchangedReason(file("a", "modified", 0, 0))).toBe("Only the file's mode changed.");
  });
});
