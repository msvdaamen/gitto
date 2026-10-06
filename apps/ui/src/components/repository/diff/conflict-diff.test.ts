import { hydratePartialDiff, UnresolvedFile } from "@pierre/diffs";
import { describe, expect, it } from "vitest";

import { conflictsLeft, markerLabel, parseConflicts, wholeSides } from "./conflict-diff";

/** A file of `lines` numbered lines, with a conflict after the `at`th, in git's merge style. */
function fileWith(lines: number, conflicts: number[], base?: string[]): string {
  const text: string[] = [];
  for (let line = 1; line <= lines; line++) {
    text.push(`line ${line}\n`);
    if (conflicts.includes(line)) {
      text.push("<<<<<<< HEAD\n", `ours ${line}\n`);
      if (base) text.push("||||||| base\n", ...base.map((b) => `${b}\n`));
      text.push("=======\n", `theirs ${line}\n`, ">>>>>>> side\n");
    }
  }
  return text.join("");
}

/** Resolves conflict `index` of `state` as the library does, through a viewer of it. */
function resolve(
  state: NonNullable<ReturnType<typeof parseConflicts>>,
  index: number,
  resolution: "current" | "incoming" | "both",
) {
  const file = new UnresolvedFile<undefined>({ onMergeConflictAction: () => undefined });
  // The conflicts are taken from the last render; the container is only needed to render.
  file.render({
    file: state.file,
    fileDiff: state.diff,
    actions: state.actions,
    markerRows: state.markerRows,
    containerWrapper: document.createElement("div"),
  });
  const resolved = file.resolveConflict(index, resolution, state.diff);
  file.cleanUp();
  return resolved!;
}

describe("parseConflicts", () => {
  it("keeps only the lines around the conflicts", () => {
    const state = parseConflicts("f.txt", fileWith(1000, [100, 900]), "f:1")!;
    expect(state.diff.isPartial).toBe(true);
    expect(state.diff.hunks).toHaveLength(2);
    // Three lines either side, and each side's line.
    expect(state.diff.additionLines).toEqual([
      "line 98\n",
      "line 99\n",
      "line 100\n",
      "theirs 100\n",
      "line 101\n",
      "line 102\n",
      "line 103\n",
      "line 898\n",
      "line 899\n",
      "line 900\n",
      "theirs 900\n",
      "line 901\n",
      "line 902\n",
      "line 903\n",
    ]);
    expect(state.diff.deletionLines).toContain("ours 900\n");
    expect(conflictsLeft(state)).toEqual([0, 1]);
  });

  it("keeps a short file whole, so the lines after its last conflict are counted", () => {
    const state = parseConflicts("f.txt", fileWith(100, [50]), "f:1")!;
    expect(state.diff.isPartial).toBe(false);
    expect(state.diff.additionLines).toHaveLength(101);
  });

  it("reads a diff3 conflict whose base is longer than the context around it", () => {
    const base = Array.from({ length: 20 }, (_, i) => `base ${i}`);
    const state = parseConflicts("f.txt", fileWith(50, [10], base), "f:1")!;
    expect(conflictsLeft(state)).toEqual([0]);
    expect(state.markerRows.map((row) => row.type)).toEqual([
      "marker-start",
      "marker-base",
      "marker-separator",
      "marker-end",
    ]);
  });

  it("has nothing for a file without conflicts", () => {
    expect(parseConflicts("f.txt", "just\ntext\n", "f:1")).toBeUndefined();
  });

  it("throws for markers that aren't finished", () => {
    expect(() => parseConflicts("f.txt", "a\n<<<<<<< HEAD\nours\n", "f:1")).toThrow();
  });
});

describe("resolving a conflict", () => {
  it("rewrites its region of the file, and leaves the others", () => {
    const contents = fileWith(20, [5, 15]);
    const state = parseConflicts("f.txt", contents, "f:1")!;

    const ours = resolve(state, 0, "current");
    expect(ours.file.contents).toBe(
      contents.replace("<<<<<<< HEAD\nours 5\n=======\ntheirs 5\n>>>>>>> side\n", "ours 5\n"),
    );
    expect(conflictsLeft({ ...state, ...ours, diff: ours.fileDiff })).toEqual([1]);
    expect(resolve(state, 1, "both").file.contents).toContain(
      "line 15\nours 15\ntheirs 15\nline 16\n",
    );
    // Names the diff anew, by what it was and what was done to it, in the highlighting cache.
    expect(ours.fileDiff.cacheKey).not.toBe(state.diff.cacheKey);
  });

  it("drops the base's lines with diff3 markers", () => {
    const state = parseConflicts("f.txt", fileWith(10, [5], ["base 5"]), "f:1")!;
    expect(resolve(state, 0, "incoming").file.contents).toBe(
      fileWith(10, []).replace("line 5\n", "line 5\ntheirs 5\n"),
    );
  });
});

describe("wholeSides", () => {
  it("fills in a partial diff with the whole of both sides", () => {
    const contents = fileWith(2000, [50]);
    const state = parseConflicts("f.txt", contents, "f:1")!;
    const files = wholeSides("f.txt", contents, "f:1");
    expect(files.oldFile?.contents).toBe(
      fileWith(2000, []).replace("line 50\n", "line 50\nours 50\n"),
    );

    const whole = hydratePartialDiff("clone", state.diff, files);
    expect(whole.isPartial).toBe(false);
    // The conflict is where it was, so its buttons and markers still are.
    expect(whole.hunks.map((hunk) => hunk.unifiedLineStart)).toEqual(
      state.diff.hunks.map((hunk) => hunk.unifiedLineStart),
    );
    expect(whole.additionLines[whole.hunks[0]!.additionLineIndex]).toBe("line 48\n");
  });
});

describe("markerLabel", () => {
  it("reads the label git gave a side", () => {
    expect(markerLabel("<<<<<<< HEAD\n")).toBe("HEAD");
    expect(markerLabel(">>>>>>> 1a2b3c4 (Fix the header)\r\n")).toBe("1a2b3c4 (Fix the header)");
    expect(markerLabel(">>>>>>>\n")).toBe("");
  });
});
