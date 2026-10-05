// Builds the patch that stages or unstages some lines of a file, from git's own patch of its
// changes: the one the user picked them from. Nothing is diffed again here; lines are only kept,
// dropped, or kept as they are on both sides, and the hunk headers counted again to match.

import { LinesNotStageableError } from "../../core/errors";
import type { LineRange, LineSelection } from "./schema";

/**
 * Which way the lines go: staged, from the unstaged changes (the index compared to the working
 * tree), or unstaged, from the staged ones (HEAD compared to the index). Either way, the patch
 * built is applied to the index: as it is to stage, in reverse to unstage.
 */
export type LineAction = "stage" | "unstage";

/** One line of a hunk, without its marker; `text` keeps a CR before the line's end, if it has one. */
interface PatchLine {
  kind: " " | "-" | "+";
  text: string;
  /** It's the last of its side of the file, without a newline ("\ No newline at end of file"). */
  noEol: boolean;
}

interface Hunk {
  oldStart: number;
  oldCount: number;
  newStart: number;
  newCount: number;
  /** What follows the second `@@`, e.g. the function the hunk is in. */
  heading: string;
  lines: PatchLine[];
}

const HUNK_HEADER = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@(.*)$/;

const NO_EOL = "\\ No newline at end of file";

/**
 * The patch that stages (or unstages, `action`) the lines `selection` picks from `patch`, git's
 * patch of one file's unstaged (or staged) changes; `undefined` if none of them are changes.
 *
 * To stage, a picked line is kept as it is; an added line that isn't picked is dropped, and a
 * removed one stays, as an unchanged line: it's still in the index. To unstage, the other way
 * around: the patch is applied in reverse, from its new side (the index) to its old side, so an
 * added line that isn't picked stays, unchanged, and a removed one is dropped.
 *
 * Only lines move. A change of the file's mode stays where it is, and so does a rename once it's
 * staged: lines are unstaged from the renamed file. An unstaged rename (of a file added with
 * `--intent-to-add`) is staged with the lines, which are only there on top of the old file's.
 * A file that's added or deleted stays so if all of it moves, and is changed otherwise.
 *
 * Rejects with `LinesNotStageableError` for a patch whose lines can't be picked: a binary file's,
 * a link's or a submodule's, one that changed type, or one without changed lines.
 */
export function linesPatch(
  patch: string,
  selection: LineSelection,
  action: LineAction,
): string | undefined {
  const verb = action === "stage" ? "staged" : "unstaged";
  const { header, hunks } = parsePatch(patch, verb);
  const picked = buildHunks(hunks, selection, action);
  if (picked.length === 0) return undefined;

  const file = parseHeader(header, verb);
  // Whether the file is still added, or deleted, as a whole: all of its lines move.
  const added = file.minus === "/dev/null" && picked.every((hunk) => hunk.oldCount === 0);
  const deleted = file.plus === "/dev/null" && picked.every((hunk) => hunk.newCount === 0);
  // Unstaging lines leaves the rename staged: they're unstaged from the renamed file.
  const renamed = file.rename.length > 0 && action === "stage";
  const plus = file.plus === "/dev/null" && !deleted ? otherSide(file.minus, "b/") : file.plus;
  const minus =
    (file.minus === "/dev/null" && !added) || (file.rename.length > 0 && !renamed)
      ? otherSide(plus, "a/")
      : file.minus;

  const lines = [
    file.rename.length > 0 && !renamed
      ? `diff --git ${withoutTab(minus)} ${withoutTab(plus)}`
      : file.diff,
  ];
  if (added && file.created) lines.push(file.created);
  if (deleted && file.deleted) lines.push(file.deleted);
  if (renamed) lines.push(...file.rename);
  lines.push(`--- ${minus}`, `+++ ${plus}`);
  for (const hunk of picked) {
    lines.push(
      `@@ -${range(hunk.oldStart, hunk.oldCount)} +${range(hunk.newStart, hunk.newCount)} @@${hunk.heading}`,
    );
    for (const line of hunk.lines) {
      lines.push(`${line.kind}${line.text}`);
      if (line.noEol) lines.push(NO_EOL);
    }
  }
  return `${lines.join("\n")}\n`;
}

/** A hunk header's range, as git writes it: without the count when it's 1. */
function range(start: number, count: number): string {
  return count === 1 ? `${start}` : `${start},${count}`;
}

/** Splits `patch` into its header's lines and its hunks. */
function parsePatch(patch: string, verb: string): { header: string[]; hunks: Hunk[] } {
  const lines = patch.split("\n");
  // The newline that ends the last line.
  if (lines.at(-1) === "") lines.pop();
  let i = 0;
  const header: string[] = [];
  while (i < lines.length && !lines[i]!.startsWith("@@ ")) header.push(lines[i++]!);
  if (header.some((line) => line.startsWith("Binary files ") || line === "GIT binary patch")) {
    throw new LinesNotStageableError(`A binary file's changes can only be ${verb} whole.`);
  }

  const hunks: Hunk[] = [];
  while (i < lines.length) {
    const match = HUNK_HEADER.exec(lines[i]!);
    // A second file, after the first one's hunks: git has a file that changed type (from a link to
    // a file, say) as one deleted and another added.
    if (!match)
      throw new LinesNotStageableError(`A file that changed type can only be ${verb} whole.`);
    i++;
    const hunk: Hunk = {
      oldStart: Number(match[1]),
      oldCount: match[2] === undefined ? 1 : Number(match[2]),
      newStart: Number(match[3]),
      newCount: match[4] === undefined ? 1 : Number(match[4]),
      heading: match[5]!,
      lines: [],
    };
    // Read by the header's counts: with `diff.suppressBlankEmpty`, an empty unchanged line is
    // left empty, without its marker.
    let oldLeft = hunk.oldCount;
    let newLeft = hunk.newCount;
    while (i < lines.length && (oldLeft > 0 || newLeft > 0 || lines[i]!.startsWith("\\"))) {
      const line = lines[i++]!;
      const marker = line[0] ?? " ";
      if (marker === "\\") {
        const last = hunk.lines.at(-1);
        if (last) last.noEol = true;
        continue;
      }
      if (marker !== " " && marker !== "-" && marker !== "+") {
        throw new Error(`Unexpected line in a hunk: ${line.slice(0, 40)}`);
      }
      if (marker !== "+") oldLeft--;
      if (marker !== "-") newLeft--;
      hunk.lines.push({ kind: marker, text: line.slice(1), noEol: false });
    }
    hunks.push(hunk);
  }
  if (hunks.length === 0) {
    throw new LinesNotStageableError(`This file has no changed lines to be ${verb}.`);
  }
  return { header, hunks };
}

/** What the lines are built from, of a patch's header. */
interface FileHeader {
  /** Its `diff --git` line. */
  diff: string;
  /** The paths on its `---` and `+++` lines, as git wrote them, or `/dev/null`. */
  minus: string;
  plus: string;
  /** Its `new file mode` or `deleted file mode` line, for a file added or deleted. */
  created: string | undefined;
  deleted: string | undefined;
  /** Its `rename from` and `rename to` lines, for a renamed file. */
  rename: string[];
}

/**
 * Reads the parts of a patch's header that are kept. The `index` line isn't: its object names are
 * of the whole file, not of the lines picked (git doesn't check them in a patch of text), and the
 * mode on it, like the `old mode` and `new mode` of a mode change, would move the mode too.
 */
function parseHeader(header: string[], verb: string): FileHeader {
  const file: FileHeader = {
    diff: header[0] ?? "",
    minus: "",
    plus: "",
    created: undefined,
    deleted: undefined,
    rename: [],
  };
  for (const line of header) {
    // A link's contents are the path it points to, and a submodule's the commit it's at.
    if (/ 1[26]0000$/.test(line)) {
      const what = line.endsWith(" 120000") ? "A link" : "A submodule";
      throw new LinesNotStageableError(`${what} can only be ${verb} whole.`);
    }
    if (line.startsWith("--- ")) file.minus = line.slice(4);
    else if (line.startsWith("+++ ")) file.plus = line.slice(4);
    else if (line.startsWith("new file mode ")) file.created = line;
    else if (line.startsWith("deleted file mode ")) file.deleted = line;
    else if (line.startsWith("rename from ") || line.startsWith("rename to "))
      file.rename.push(line);
  }
  if (!file.diff.startsWith("diff --git ") || !file.minus || !file.plus) {
    throw new Error("The patch has no file header.");
  }
  return file;
}

/**
 * `path` from a `---` or `+++` line (`a/x` or `b/x`, or quoted, `"a/x\ty"`), as the other side has
 * it: with `prefix`.
 */
function otherSide(path: string, prefix: "a/" | "b/"): string {
  const quoted = path.startsWith('"');
  const name = quoted ? path.slice(1) : path;
  if (!name.startsWith("a/") && !name.startsWith("b/")) {
    throw new Error(`Unexpected path in a patch: ${path}`);
  }
  return `${quoted ? '"' : ""}${prefix}${name.slice(2)}`;
}

/**
 * A path from a `---` or `+++` line without the tab git ends one with that has a space, as on the
 * `diff --git` line.
 */
function withoutTab(path: string): string {
  return path.endsWith("\t") ? path.slice(0, -1) : path;
}

/** The hunks with the lines picked, renumbered, without the ones left with no changes. */
function buildHunks(hunks: Hunk[], selection: LineSelection, action: LineAction): Hunk[] {
  const deletions = new Picked(selection.deletions);
  const additions = new Picked(selection.additions);
  const built: Hunk[] = [];
  // How many more lines the new side of the hunks built so far has than the old one.
  let grown = 0;
  for (const hunk of hunks) {
    const lines: PatchLine[] = [];
    let oldLine = hunk.oldStart;
    let newLine = hunk.newStart;
    for (const line of hunk.lines) {
      if (line.kind === " ") {
        oldLine++;
        newLine++;
        lines.push(line);
      } else if (line.kind === "-") {
        if (deletions.has(oldLine++)) lines.push(line);
        else if (action === "stage") lines.push({ ...line, kind: " " });
      } else {
        if (additions.has(newLine++)) lines.push(line);
        else if (action === "unstage") lines.push({ ...line, kind: " " });
      }
    }
    if (lines.every((line) => line.kind === " ")) continue;

    const ended = endOfFile(lines);
    const oldCount = ended.filter((line) => line.kind !== "+").length;
    const newCount = ended.filter((line) => line.kind !== "-").length;
    // The side the patch is applied to keeps its lines, and their numbers; the other side's are
    // moved by the lines the hunks before added or removed. A side without lines in the hunk is
    // numbered by the line before it.
    let oldStart: number;
    let newStart: number;
    if (action === "stage") {
      oldStart = hunk.oldStart;
      const before = hunk.oldStart - (hunk.oldCount > 0 ? 1 : 0) + grown;
      newStart = before + (newCount > 0 ? 1 : 0);
    } else {
      newStart = hunk.newStart;
      const before = hunk.newStart - (hunk.newCount > 0 ? 1 : 0) - grown;
      oldStart = before + (oldCount > 0 ? 1 : 0);
    }
    grown += newCount - oldCount;
    built.push({ oldStart, oldCount, newStart, newCount, heading: hunk.heading, lines: ended });
  }
  return built;
}

/**
 * `lines` with a newline at the end of any line that's no longer the last of its side: one without
 * ends the file. That's a removed line kept as an unchanged one (to stage) with added ones picked
 * after it, say: it's still the last of the old side, but the added lines follow it on the new
 * side, so it's removed without a newline and added with one. The side the patch is applied to
 * never changes this way, as all of its lines are kept.
 */
function endOfFile(lines: PatchLine[]): PatchLine[] {
  const ended: PatchLine[] = [];
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]!;
    if (!line.noEol) {
      ended.push(line);
      continue;
    }
    const after = lines.slice(i + 1);
    const oldAfter = after.some((next) => next.kind !== "+");
    const newAfter = after.some((next) => next.kind !== "-");
    if (line.kind === "-") ended.push({ ...line, noEol: !oldAfter });
    else if (line.kind === "+") ended.push({ ...line, noEol: !newAfter });
    else if (oldAfter === newAfter) ended.push({ ...line, noEol: !oldAfter });
    else {
      ended.push({ kind: "-", text: line.text, noEol: !oldAfter });
      ended.push({ kind: "+", text: line.text, noEol: !newAfter });
    }
  }
  return ended;
}

/**
 * Whether a line is in some ranges, for line numbers asked in increasing order: in one pass over
 * the ranges, however many lines and ranges there are.
 */
class Picked {
  private readonly ranges: LineRange[];
  private next = 0;

  constructor(ranges: LineRange[]) {
    this.ranges = ranges.toSorted((a, b) => a.start - b.start);
  }

  has(line: number): boolean {
    while (this.next < this.ranges.length && this.ranges[this.next]!.end < line) this.next++;
    // Ranges can overlap: a later one can end after a longer one that starts before it.
    for (let i = this.next; i < this.ranges.length && this.ranges[i]!.start <= line; i++) {
      if (this.ranges[i]!.end >= line) return true;
    }
    return false;
  }
}
