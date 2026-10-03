import type { Stash, StatusSummary } from "@gitto/git/types";
import { describe, expect, it } from "vitest";

import { popTitle, stashTitle } from "./stash";

const status: Pick<StatusSummary, "head" | "counts"> = {
  head: { kind: "branch", name: "main", sha: "abc" },
  counts: { files: 3, staged: 1, unstaged: 2, conflicted: 0 },
};
const clean = { files: 0, staged: 0, unstaged: 0, conflicted: 0 };
const conflicted = { files: 1, staged: 0, unstaged: 0, conflicted: 1 };
const stash: Stash = { sha: "def", message: "WIP on main: abc first", createdAt: 0 };

describe("stashTitle", () => {
  it("says how many files would be stashed, or why they can't be", () => {
    expect(stashTitle(status)).toBe("Stash 3 changed files");
    expect(stashTitle({ ...status, counts: { ...clean, files: 1, unstaged: 1 } })).toBe(
      "Stash 1 changed file",
    );
    expect(stashTitle({ ...status, counts: clean })).toBe("No changes to stash.");
    expect(stashTitle({ ...status, counts: conflicted })).toBe(
      "Resolve the conflicts before stashing.",
    );
    expect(stashTitle({ ...status, head: { kind: "unborn", name: "main" } })).toBe(
      "Make the first commit before stashing changes.",
    );
  });
});

describe("popTitle", () => {
  it("names the stash that would be popped, or says why none can be", () => {
    expect(popTitle(status, [stash, { ...stash, message: "older" }])).toBe(
      'Pop "WIP on main: abc first"',
    );
    // Local changes don't stop a pop: git refuses only if the stash would overwrite them.
    expect(popTitle({ counts: clean }, [stash])).toBe('Pop "WIP on main: abc first"');
    expect(popTitle(status, [])).toBe("No stashes to pop.");
    expect(popTitle({ counts: conflicted }, [stash])).toBe(
      "Resolve the conflicts before popping a stash.",
    );
  });
});
