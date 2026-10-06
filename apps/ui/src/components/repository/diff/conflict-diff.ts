// A conflicted file's text, conflict markers and all, as the library's diff of its sides: ours as
// the deletions, theirs as the additions, and the base's lines (diff3, zdiff3) as unchanged ones.
import {
  UnresolvedFile,
  type FileContents,
  type FileDiffLoadedFiles,
  type FileDiffMetadata,
  type MergeConflictMarkerRow,
  type UnresolvedFileRenderProps,
} from "@pierre/diffs";

/** A conflict as the library has it: where it is in the diff, and its markers. */
export type ConflictAction = NonNullable<
  NonNullable<UnresolvedFileRenderProps<undefined>["actions"]>[number]
>;

/**
 * Reads conflict markers as the library's `UnresolvedFile` does, which keeps how to itself: it
 * parses a file it's given the first time, which this has it do each time, without showing it.
 */
class ConflictParser extends UnresolvedFile<undefined> {
  constructor() {
    // Told what's resolved, rather than resolving by itself: it then takes the parsed state back.
    super({ onMergeConflictAction: () => undefined });
  }

  parse(file: FileContents, maxContextLines: number) {
    this.options.maxContextLines = maxContextLines;
    this.dropParsed();
    try {
      const parsed = this["getOrComputeDiff"]({
        file,
        fileDiff: undefined,
        actions: undefined,
        markerRows: undefined,
      });
      if (!parsed) throw new Error("The file's conflicts couldn't be read.");
      return parsed;
    } finally {
      // Not kept: it's the whole file, and its whole diff, which the parser would hold for as
      // long as the app runs.
      this.dropParsed();
    }
  }

  /** Lets go of the file last parsed, which it keeps to parse it only once. */
  private dropParsed() {
    this.computedCache = {
      file: undefined,
      fileDiff: undefined,
      actions: undefined,
      markerRows: undefined,
    };
  }
}

let parser: ConflictParser | undefined;

/** The library's diff of `file`'s conflicts, whole; throws if its markers can't be read. */
function parse(file: FileContents, maxContextLines: number) {
  return (parser ??= new ConflictParser()).parse(file, maxContextLines);
}

/** Unchanged lines shown around each conflict, as many as git shows around a change. */
export const CONTEXT_LINES = 3;

/**
 * Lines (of both sides) from which a file's conflicts are shown with only the lines around them,
 * as a patch is, rather than with the whole file: see `parseConflicts`. Fewer highlight whole in
 * well under the time the view waits for that (see `HIGHLIGHT_WAIT_MS`).
 */
const PARTIAL_FROM_LINES = 500;

/** A conflicted file's text, and what the library shows of it: its diff, conflicts and markers. */
export interface ConflictState {
  /** The text on disk, or as it'll be written, with what's left of the markers. */
  file: FileContents;
  diff: FileDiffMetadata;
  /** Each conflict, by its index; `undefined` once it's resolved. */
  actions: (ConflictAction | undefined)[];
  markerRows: MergeConflictMarkerRow[];
}

/**
 * `contents` as the library's diff of its conflicts, named `cacheKey` in the highlighting cache;
 * `undefined` if it has none. Throws if its markers can't be read as conflicts, e.g. one isn't
 * finished.
 *
 * Partial for a long file, with only the lines around the conflicts, like a patch's diff: the
 * library highlights all of a whole file's lines, both sides, which takes a second for a few
 * thousand, and again with each conflict resolved. The rest is filled in when it's asked for (see
 * `wholeSides`). A short one is whole, so the lines after its last conflict are counted.
 */
export function parseConflicts(
  name: string,
  contents: string,
  cacheKey: string,
): ConflictState | undefined {
  // The library splits a conflict whose base has more lines than twice the context into two hunks,
  // which it then can't show; the context is made to fit the longest.
  const context = Math.max(CONTEXT_LINES, Math.ceil(longestBase(contents) / 2));
  const file = { name, contents, cacheKey };
  const parsed = parse(file, context);
  if (parsed.actions.length === 0) return undefined;
  const { additionLines, deletionLines } = parsed.fileDiff;
  const long = additionLines.length + deletionLines.length >= PARTIAL_FROM_LINES;
  return {
    file,
    diff: long ? toPartial(parsed.fileDiff) : parsed.fileDiff,
    actions: parsed.actions,
    markerRows: parsed.markerRows,
  };
}

/** How many lines the longest base section has, between `|||||||` and `=======`. */
function longestBase(contents: string): number {
  let longest = 0;
  let start: number | undefined;
  const lines = contents.split("\n");
  for (const [index, line] of lines.entries()) {
    if (/^\|{7}(\s|$)/.test(line)) start = index;
    else if (start !== undefined && /^={7}\r?$/.test(line)) {
      longest = Math.max(longest, index - start - 1);
      start = undefined;
    }
  }
  return longest;
}

/**
 * `diff`, a whole file's, with only the lines its hunks show, as a patch's diff has them: their
 * indexes are into those lines, and it ends with its last hunk.
 */
function toPartial(diff: FileDiffMetadata): FileDiffMetadata {
  const additionLines: string[] = [];
  const deletionLines: string[] = [];
  const hunks = diff.hunks.map((hunk) => {
    const at = { additionLineIndex: additionLines.length, deletionLineIndex: deletionLines.length };
    const hunkContent = hunk.hunkContent.map((content) => {
      const moved = {
        ...content,
        additionLineIndex: additionLines.length,
        deletionLineIndex: deletionLines.length,
      };
      const [additions, deletions] =
        content.type === "context"
          ? [content.lines, content.lines]
          : [content.additions, content.deletions];
      additionLines.push(
        ...diff.additionLines.slice(
          content.additionLineIndex,
          content.additionLineIndex + additions,
        ),
      );
      deletionLines.push(
        ...diff.deletionLines.slice(
          content.deletionLineIndex,
          content.deletionLineIndex + deletions,
        ),
      );
      return moved;
    });
    return { ...hunk, ...at, hunkContent };
  });
  // The lines after the last hunk aren't counted until they're filled in.
  const last = diff.hunks.at(-1);
  return {
    ...diff,
    isPartial: true,
    hunks,
    additionLines,
    deletionLines,
    unifiedLineCount: last ? last.unifiedLineStart + last.unifiedLineCount : 0,
    splitLineCount: last ? last.splitLineStart + last.splitLineCount : 0,
  };
}

/**
 * Past this many lines of conflicts and the lines around them, a file is shown whole rather than
 * conflict by conflict: the library draws all of those at once, not just the ones on screen.
 */
const MAX_CONFLICT_LINES = 10_000;

/**
 * `contents`' conflicts, to show one by one (see `parseConflicts`); or, if they can't be, why: the
 * file is shown whole then, to edit by hand. Neither for a file without conflicts.
 */
export function readConflicts(
  name: string,
  contents: string,
  cacheKey: string,
): { state?: ConflictState; problem?: string } {
  let state: ConflictState | undefined;
  try {
    state = parseConflicts(name, contents, cacheKey);
  } catch {
    return {
      problem: "This file's conflict markers can't be read as conflicts. Edit them by hand.",
    };
  }
  if (state && drawnLines(state.diff) > MAX_CONFLICT_LINES) {
    return {
      problem:
        "This file's conflicts are too long to show one by one. Edit it by hand, or keep one side.",
    };
  }
  return { state };
}

/** How many lines the library draws of `diff`: its hunks', not those between them. */
function drawnLines(diff: FileDiffMetadata): number {
  return diff.hunks.reduce((lines, hunk) => lines + hunk.unifiedLineCount, 0);
}

/**
 * Both sides of `contents` in full, ours and theirs, the base's lines in both: what fills in its
 * partial diff (see `parseConflicts`), or one with some of its conflicts resolved since, as their
 * lines are both sides' then.
 */
export function wholeSides(name: string, contents: string, cacheKey: string): FileDiffLoadedFiles {
  const { fileDiff } = parse({ name, contents }, Math.max(CONTEXT_LINES, longestBase(contents)));
  return {
    oldFile: { name, contents: fileDiff.deletionLines.join(""), cacheKey: `${cacheKey}:ours` },
    newFile: { name, contents: fileDiff.additionLines.join(""), cacheKey: `${cacheKey}:theirs` },
  };
}

/**
 * Names a conflicted file's text in the highlighting cache: by its version on disk, which names
 * its bytes, or else by what's in it.
 */
export function conflictsCacheKey(
  fileKey: string,
  contents: string,
  version: string | null,
): string {
  return `${fileKey}:conflicts:${version ?? `edited:${contents.length}:${hash(contents)}`}`;
}

/** A short hash of `text`, to name edits whose version on disk isn't known yet. */
function hash(text: string): string {
  let value = 0;
  for (let i = 0; i < text.length; i++) value = (Math.imul(31, value) + text.charCodeAt(i)) | 0;
  return (value >>> 0).toString(36);
}

/** The conflicts left, by their indexes, in the order they're in the file. */
export function conflictsLeft(state: ConflictState | undefined): number[] {
  if (!state) return [];
  return state.actions.flatMap((action, index) => (action ? [index] : []));
}

/** The label git gave a side in a conflict's markers, e.g. `HEAD`, or `side`. */
export function markerLabel(marker: string): string {
  return marker.replace(/^[<>|]{7,}\s?/, "").trim();
}
