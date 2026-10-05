// Which changed lines of a diff on show are picked to stage or unstage: from a selection of its
// rows, or a hunk, and where the keyboard moves a selection to. Read from the diff the viewer
// parsed from git's patch, by the same line numbers the patch's hunk headers count.

import type { LineRange, LineSelection } from "@gitto/git/types";
import type {
  DiffLineAnnotation,
  FileDiffMetadata,
  SelectedLineRange,
  SelectionSide,
} from "@pierre/diffs";

/** A removed or added line of a diff. */
export interface ChangedLine {
  /** Removed lines are on the deletions side, numbered in the old file; added ones in the new. */
  side: SelectionSide;
  lineNumber: number;
  /** The index of its hunk. */
  hunk: number;
  /**
   * Where it is in the old and the new file: a removed line is before the new side's line that
   * follows it, an added one before the old side's.
   */
  old: number;
  new: number;
}

/** Every removed and added line of `diff`, in the order they're in: by hunk, removed ones first. */
export function changedLines(diff: FileDiffMetadata): ChangedLine[] {
  const lines: ChangedLine[] = [];
  for (const [index, hunk] of diff.hunks.entries()) {
    // A side without lines in the hunk is numbered by the line before it.
    let old = hunk.deletionCount > 0 ? hunk.deletionStart : hunk.deletionStart + 1;
    let next = hunk.additionCount > 0 ? hunk.additionStart : hunk.additionStart + 1;
    for (const content of hunk.hunkContent) {
      if (content.type === "context") {
        old += content.lines;
        next += content.lines;
        continue;
      }
      for (let i = 0; i < content.deletions; i++) {
        lines.push({
          side: "deletions",
          lineNumber: old + i,
          hunk: index,
          old: old + i,
          new: next,
        });
      }
      const after = old + content.deletions;
      for (let i = 0; i < content.additions; i++) {
        lines.push({
          side: "additions",
          lineNumber: next + i,
          hunk: index,
          old: after,
          new: next + i,
        });
      }
      old = after;
      next += content.additions;
    }
  }
  return lines;
}

/** The row a line is on in the view, unified or split; `undefined` if it isn't on one. */
export type RowOf = (lineNumber: number, side: SelectionSide) => number | undefined;

/** The changed lines in the rows a selection spans, in either direction. */
export function selectedLines(
  lines: ChangedLine[],
  range: SelectedLineRange,
  rowOf: RowOf,
): ChangedLine[] {
  const start = rowOf(range.start, range.side ?? "additions");
  const end = rowOf(range.end, range.endSide ?? range.side ?? "additions");
  if (start === undefined || end === undefined) return [];
  const [top, bottom] = start <= end ? [start, end] : [end, start];
  return lines.filter((line) => {
    const row = rowOf(line.lineNumber, line.side);
    return row !== undefined && row >= top && row <= bottom;
  });
}

/** `lines` as the ranges of line numbers on each side that the git package takes. */
export function toSelection(lines: ChangedLine[]): LineSelection {
  const ranges = (side: SelectionSide) => {
    const numbers = lines
      .filter((line) => line.side === side)
      .map((line) => line.lineNumber)
      .toSorted((a, b) => a - b);
    const result: LineRange[] = [];
    for (const number of numbers) {
      const last = result.at(-1);
      if (last && number <= last.end + 1) last.end = Math.max(last.end, number);
      else result.push({ start: number, end: number });
    }
    return result;
  };
  return { deletions: ranges("deletions"), additions: ranges("additions") };
}

/**
 * Rows with changed lines, in order, each with the first of its lines: in split view a row can
 * have a removed line and an added one, side by side. What the keyboard moves through.
 */
export function changedRows(
  lines: ChangedLine[],
  rowOf: RowOf,
): { row: number; line: ChangedLine }[] {
  const rows = new Map<number, ChangedLine>();
  for (const line of lines) {
    const row = rowOf(line.lineNumber, line.side);
    if (row !== undefined && !rows.has(row)) rows.set(row, line);
  }
  return [...rows].map(([row, line]) => ({ row, line })).toSorted((a, b) => a.row - b.row);
}

/** A selection of the rows from `from`'s to `to`'s. */
export function rangeOf(from: ChangedLine, to: ChangedLine = from): SelectedLineRange {
  return { start: from.lineNumber, side: from.side, end: to.lineNumber, endSide: to.side };
}

/** What a hunk's button is put on the view with: its hunk. */
export interface HunkButton {
  hunk: number;
}

/**
 * Where each hunk's button goes: below the unchanged line before its first change, which puts it
 * above the change; or above the first line of the file, for a hunk that starts with a change
 * (only at the top of the file, where there's no unchanged line before it).
 */
export function hunkButtons(diff: FileDiffMetadata): DiffLineAnnotation<HunkButton>[] {
  const buttons: DiffLineAnnotation<HunkButton>[] = [];
  for (const [hunk, { hunkContent, additionStart, additionCount }] of diff.hunks.entries()) {
    const [first] = hunkContent;
    if (first?.type === "context" && first.lines > 0) {
      const start = additionCount > 0 ? additionStart : additionStart + 1;
      buttons.push({ side: "additions", lineNumber: start + first.lines - 1, metadata: { hunk } });
    } else if (hunk === 0 && first?.type === "change") {
      const side = first.additions > 0 ? "additions" : "deletions";
      buttons.push({ side, lineNumber: 0, metadata: { hunk } });
    }
  }
  return buttons;
}
