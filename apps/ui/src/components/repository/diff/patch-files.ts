// Fitting whole files to a patch git gave, with types only from @pierre/diffs, which the viewer
// loads on its own.
import type { FileDiffMetadata, HunkExpansionRegion } from "@pierre/diffs";

export type Side = "old" | "new";

/**
 * `contents`, if they're the file `diff` is a patch of on its `side`: each of its hunks' lines is
 * where the patch has it. A file in the working tree can have changed since its patch was read, and
 * one with CRLF line ends throughout is compared with LF ones (git's `core.autocrlf`), which the
 * contents are changed to then (see `asLf`). Throws if they aren't.
 */
export function fitToPatch(diff: FileDiffMetadata, side: Side, contents: string): string {
  if (hasPatchLines(diff, side, contents)) return contents;
  const lf = asLf(contents);
  if (lf !== undefined && hasPatchLines(diff, side, lf)) return lf;
  throw changedError(contents);
}

/**
 * `contents` with LF line ends, if they have CRLF ones throughout, which saving them makes CRLF
 * again (see `saveWorkingTreeFile`); `undefined` otherwise. A file that mixes them isn't taken
 * with LF ones: its line ends couldn't be told apart again, and saving it would change them all.
 */
function asLf(contents: string): string | undefined {
  if (!contents.includes("\r\n") || /(^|[^\r])\n/.test(contents)) return undefined;
  return contents.replaceAll("\r\n", "\n");
}

function changedError(contents: string): Error {
  return new Error(
    contents.includes("\r\n") && /(^|[^\r])\n/.test(contents)
      ? "This file mixes CRLF and LF line ends, which git shows as all LF here."
      : "The file has changed since its changes were loaded. Try again in a moment.",
  );
}

/**
 * `contents`, if they're all of `lines`, the new side of a patch that has the whole file (an added
 * one), with LF line ends for CRLF ones as `fitToPatch` takes them. Throws if they aren't.
 */
export function fitToLines(lines: readonly string[], contents: string): string {
  const whole = lines.join("");
  if (contents === whole) return contents;
  if (asLf(contents) === whole) return whole;
  throw changedError(contents);
}

/** Whether `contents` have the lines of each of `diff`'s hunks on its `side`, where it has them. */
function hasPatchLines(diff: FileDiffMetadata, side: Side, contents: string): boolean {
  const patchLines = side === "old" ? diff.deletionLines : diff.additionLines;
  // The line `line` (from 0) of `contents` starts at `offset`.
  let line = 0;
  let offset = 0;
  for (const hunk of diff.hunks) {
    const start = side === "old" ? hunk.deletionStart : hunk.additionStart;
    const count = side === "old" ? hunk.deletionCount : hunk.additionCount;
    let index = side === "old" ? hunk.deletionLineIndex : hunk.additionLineIndex;
    if (count === 0) continue;
    for (; line < start - 1; line++) {
      const end = contents.indexOf("\n", offset);
      if (end === -1) return false;
      offset = end + 1;
    }
    for (const last = index + count; index < last; index++, line++) {
      // A line keeps its newline, as in the patch; the last one may have none.
      const end = contents.indexOf("\n", offset);
      const next = end === -1 ? contents.length : end + 1;
      if (offset === next || contents.slice(offset, next) !== patchLines[index]) return false;
      offset = next;
    }
  }
  return true;
}

/**
 * Whether the hydrated `diff` (see `hydratePartialDiff`) has as many unchanged lines after its last
 * hunk in both files, as the viewer takes for granted (and throws otherwise): a file in the
 * working tree that got lines at its end since its patch was read doesn't.
 */
export function hasMatchingEnds(diff: FileDiffMetadata): boolean {
  const last = diff.hunks.at(-1);
  if (!last) return true;
  return (
    diff.additionLines.length - sideEnd(last.additionStart, last.additionCount) ===
    diff.deletionLines.length - sideEnd(last.deletionStart, last.deletionCount)
  );
}

/**
 * The unchanged lines that were shown around `from`'s hunks (`expanded`, by hunk, as the viewer
 * keeps them), around `to`'s instead: both hydrated diffs of the same file, `to` with changes made
 * since. They're found by their lines in the old file, which a change to the new one leaves alone:
 * the index, or HEAD, for uncommitted changes.
 */
export function carriedExpansion(
  from: FileDiffMetadata,
  expanded: ReadonlyMap<number, HunkExpansionRegion>,
  to: FileDiffMetadata,
): Map<number, HunkExpansionRegion> {
  // The old file's lines that were shown, as [start, end) ranges.
  const shown: [number, number][] = [];
  for (const gap of collapsedGaps(from)) {
    const region = expanded.get(gap.index);
    if (!region) continue;
    if (region.fromStart > 0) shown.push([gap.start, gap.start + region.fromStart]);
    if (region.fromEnd > 0) shown.push([gap.end - region.fromEnd, gap.end]);
  }
  const carried = new Map<number, HunkExpansionRegion>();
  if (shown.length === 0) return carried;
  for (const gap of collapsedGaps(to)) {
    let fromStart = 0;
    let fromEnd = 0;
    for (const [start, end] of shown) {
      if (end <= gap.start || start >= gap.end) continue;
      if (start <= gap.start) fromStart = Math.max(fromStart, Math.min(end, gap.end) - gap.start);
      if (end >= gap.end) fromEnd = Math.max(fromEnd, gap.end - Math.max(start, gap.start));
    }
    if (fromStart > 0 || fromEnd > 0) carried.set(gap.index, { fromStart, fromEnd });
  }
  return carried;
}

/**
 * The unchanged lines hidden before each of the hydrated `diff`'s hunks, and after the last (as
 * hunk `hunks.length`, as the viewer has it), as [start, end) in the old file.
 */
function collapsedGaps(diff: FileDiffMetadata): { index: number; start: number; end: number }[] {
  const gaps = diff.hunks.map((hunk, index) => {
    const end = sideStart(hunk.deletionStart, hunk.deletionCount);
    return { index, start: end - Math.max(hunk.collapsedBefore, 0), end };
  });
  const last = diff.hunks.at(-1);
  if (last) {
    const start = sideEnd(last.deletionStart, last.deletionCount);
    gaps.push({ index: diff.hunks.length, start, end: diff.deletionLines.length });
  }
  return gaps.filter((gap) => gap.end > gap.start);
}

/** Where a hunk starts in one of the files, from 0: one with no lines there starts after `start`. */
function sideStart(start: number, count: number): number {
  return count === 0 ? start : start - 1;
}

function sideEnd(start: number, count: number): number {
  return sideStart(start, count) + count;
}
