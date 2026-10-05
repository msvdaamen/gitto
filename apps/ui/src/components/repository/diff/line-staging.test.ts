import { FileDiff, parsePatchFiles, type FileDiffMetadata } from "@pierre/diffs";
import { describe, expect, it } from "vitest";

import {
  changedLines,
  changedRows,
  hunkButtons,
  rangeOf,
  selectedLines,
  toSelection,
  type RowOf,
} from "./line-staging";

/** The rows lines are on, as the viewer numbers them, in unified or split view. */
class Rows extends FileDiff {
  of(diff: FileDiffMetadata, style: "unified" | "split"): RowOf {
    return (lineNumber, side) =>
      this.getLineIndexForDiff(diff, lineNumber, side)?.[style === "split" ? 1 : 0];
  }
}

function parse(...lines: string[]): FileDiffMetadata {
  return parsePatchFiles(`${lines.join("\n")}\n`, "test")[0]!.files[0]!;
}

const HEADER = ["diff --git a/f.txt b/f.txt", "--- a/f.txt", "+++ b/f.txt"];

// Two hunks: in the first, 3 replaced by two lines; in the second, 12 removed.
const DIFF = parse(
  ...HEADER,
  "@@ -1,5 +1,6 @@",
  " 1",
  " 2",
  "-3",
  "+three",
  "+3b",
  " 4",
  " 5",
  "@@ -10,5 +11,4 @@",
  " 10",
  " 11",
  "-12",
  " 13",
  " 14",
);

describe("changedLines", () => {
  it("lists removed and added lines, with where they are on both sides", () => {
    expect(changedLines(DIFF)).toEqual([
      { side: "deletions", lineNumber: 3, hunk: 0, old: 3, new: 3 },
      { side: "additions", lineNumber: 3, hunk: 0, old: 4, new: 3 },
      { side: "additions", lineNumber: 4, hunk: 0, old: 4, new: 4 },
      { side: "deletions", lineNumber: 12, hunk: 1, old: 12, new: 13 },
    ]);
  });

  it("numbers the lines of a new file from 1", () => {
    const added = parse(
      "diff --git a/f.txt b/f.txt",
      "new file mode 100644",
      "--- /dev/null",
      "+++ b/f.txt",
      "@@ -0,0 +1,2 @@",
      "+a",
      "+b",
    );
    expect(changedLines(added).map((line) => [line.side, line.lineNumber])).toEqual([
      ["additions", 1],
      ["additions", 2],
    ]);
  });
});

describe("selectedLines", () => {
  const lines = changedLines(DIFF);
  const rows = new Rows();

  it("picks the changed lines in the rows selected, in either direction", () => {
    const rowOf = rows.of(DIFF, "unified");
    // From the unchanged 2 to the added 3b: 3, three and 3b.
    const down = { start: 2, side: "additions", end: 4, endSide: "additions" } as const;
    expect(toSelection(selectedLines(lines, down, rowOf))).toEqual({
      deletions: [{ start: 3, end: 3 }],
      additions: [{ start: 3, end: 4 }],
    });
    const up = { start: 4, side: "additions", end: 3, endSide: "deletions" } as const;
    expect(toSelection(selectedLines(lines, up, rowOf))).toEqual({
      deletions: [{ start: 3, end: 3 }],
      additions: [{ start: 3, end: 4 }],
    });
    // Only unchanged lines.
    expect(selectedLines(lines, { start: 1, side: "additions", end: 2 }, rowOf)).toEqual([]);
  });

  it("picks both sides of the rows selected in split view", () => {
    // The row with 3 removed and three added, selected on its added side.
    const range = { start: 3, side: "additions", end: 3 } as const;
    expect(toSelection(selectedLines(lines, range, rows.of(DIFF, "split")))).toEqual({
      deletions: [{ start: 3, end: 3 }],
      additions: [{ start: 3, end: 3 }],
    });
    // In unified view, only the added line.
    expect(toSelection(selectedLines(lines, range, rows.of(DIFF, "unified")))).toEqual({
      deletions: [],
      additions: [{ start: 3, end: 3 }],
    });
  });

  it("picks lines across hunks", () => {
    const range = { start: 4, side: "additions", end: 12, endSide: "deletions" } as const;
    expect(toSelection(selectedLines(lines, range, rows.of(DIFF, "unified")))).toEqual({
      deletions: [{ start: 12, end: 12 }],
      additions: [{ start: 4, end: 4 }],
    });
  });
});

/** A line by its side and number, as `changedRows` has them. */
const line = (side: string, lineNumber: number) => ({ side, lineNumber });

describe("changedRows", () => {
  it("lists each row with changes once, in order", () => {
    const rows = new Rows();
    const lines = changedLines(DIFF);
    expect(
      changedRows(lines, rows.of(DIFF, "unified")).map(({ line: { side, lineNumber } }) =>
        line(side, lineNumber),
      ),
    ).toEqual([
      line("deletions", 3),
      line("additions", 3),
      line("additions", 4),
      line("deletions", 12),
    ]);
    // Side by side, 3 and three are on one row.
    expect(
      changedRows(lines, rows.of(DIFF, "split")).map(({ line: { side, lineNumber } }) =>
        line(side, lineNumber),
      ),
    ).toEqual([line("deletions", 3), line("additions", 4), line("deletions", 12)]);
  });
});

describe("toSelection", () => {
  it("joins lines next to each other into ranges", () => {
    const lines = changedLines(DIFF);
    expect(toSelection(lines)).toEqual({
      deletions: [
        { start: 3, end: 3 },
        { start: 12, end: 12 },
      ],
      additions: [{ start: 3, end: 4 }],
    });
  });
});

describe("rangeOf", () => {
  it("selects the rows from one line to another", () => {
    const [removed, , added] = changedLines(DIFF);
    expect(rangeOf(removed!, added)).toEqual({
      start: 3,
      side: "deletions",
      end: 4,
      endSide: "additions",
    });
  });
});

describe("hunkButtons", () => {
  it("puts each hunk's button below the unchanged line before its first change", () => {
    expect(hunkButtons(DIFF)).toEqual([
      { side: "additions", lineNumber: 2, metadata: { hunk: 0 } },
      { side: "additions", lineNumber: 12, metadata: { hunk: 1 } },
    ]);
  });

  it("puts it above the file for a hunk that starts with a change", () => {
    const top = parse(...HEADER, "@@ -1,2 +1,2 @@", "-1", "+one", " 2");
    expect(hunkButtons(top)).toEqual([{ side: "additions", lineNumber: 0, metadata: { hunk: 0 } }]);
    const deleted = parse(
      "diff --git a/f.txt b/f.txt",
      "deleted file mode 100644",
      "--- a/f.txt",
      "+++ /dev/null",
      "@@ -1,2 +0,0 @@",
      "-a",
      "-b",
    );
    expect(hunkButtons(deleted)).toEqual([
      { side: "deletions", lineNumber: 0, metadata: { hunk: 0 } },
    ]);
  });
});
