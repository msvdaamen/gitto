// Conflict markers, as git leaves them in a file it couldn't merge: `<<<<<<< ours`, then the base's
// lines after `||||||| base` with the diff3 and zdiff3 styles, then `=======`, then `>>>>>>> theirs`.

/** How many bytes from its start git looks at to tell whether a file is binary: for a NUL. */
export const BINARY_CHECK_BYTES = 8000;

/** Whether git takes `bytes` for a binary file's, as it does when it merges: not text to mark. */
export function isBinary(bytes: Uint8Array): boolean {
  return bytes.subarray(0, BINARY_CHECK_BYTES).includes(0);
}

/**
 * How many conflicts `text` still has: regions from a start marker to an end marker, with a
 * separator between. A marker alone isn't one, nor are markers of another length than 7 or more
 * (git's own, unless `conflict-marker-size` says otherwise), so a file that only mentions them, or
 * a heading underlined with `=`, has none. Conflicts inside conflicts, which a recursive merge can
 * leave, count once each.
 */
export function countConflicts(text: string): number {
  const counter = new ConflictCounter();
  counter.push(text);
  return counter.end();
}

/**
 * Past this many characters, a line's rest isn't kept while the next piece of it comes: a marker is
 * told by its start, and a separator line this long isn't one.
 */
const KEPT_LINE = 4096;

/**
 * Counts conflicts (see `countConflicts`) in text given a piece at a time, as a file is read: one of
 * hundreds of megabytes is never all in memory, nor one string, which V8 can't make that long.
 */
export class ConflictCounter {
  private count = 0;
  /** Whether each open conflict, innermost last, has had its separator. */
  private readonly open: boolean[] = [];
  /** The start of a line whose end hasn't come yet. */
  private rest = "";

  /** Counts the conflicts in the next piece of the text. */
  push(piece: string): void {
    const text = this.rest + piece;
    const last = text.lastIndexOf("\n");
    if (last === -1) {
      // Too long to be a marker's line but for its start; and no separator, with this after it.
      this.rest = text.length > KEPT_LINE ? `${text.slice(0, KEPT_LINE)}x` : text;
      return;
    }
    this.scan(text, last);
    this.rest = text.slice(last + 1, last + 1 + KEPT_LINE + 1);
  }

  /** The conflicts in all of the text given, which has ended. */
  end(): number {
    if (this.rest) this.scan(this.rest, this.rest.length);
    this.rest = "";
    return this.count;
  }

  /** Reads the lines of `text` up to `to`, the newline of its last whole one, or its end. */
  private scan(text: string, to: number): void {
    for (let start = 0; start <= to && start < text.length;) {
      let end = text.indexOf("\n", start);
      if (end === -1 || end > to) end = to;
      // Only a line that starts with a marker's character is looked at closer.
      const first = text.charCodeAt(start);
      if (first === LT || first === EQ || first === GT) {
        const kind = markerKind(text, start, end);
        if (kind === "start") this.open.push(false);
        else if (kind === "separator" && this.open.length > 0) {
          this.open[this.open.length - 1] = true;
        } else if (kind === "end" && this.open.length > 0 && this.open.pop()) this.count++;
      }
      start = end + 1;
    }
  }
}

const LT = "<".charCodeAt(0);
const EQ = "=".charCodeAt(0);
const GT = ">".charCodeAt(0);

/** Which marker the line from `start` to `end` (its newline) is, if it's one. */
function markerKind(
  text: string,
  start: number,
  end: number,
): "start" | "separator" | "end" | undefined {
  // Without the line's CR, for a file with CRLF line ends.
  const last = text.charCodeAt(end - 1) === CR ? end - 1 : end;
  const char = text.charCodeAt(start);
  let run = start;
  while (run < last && text.charCodeAt(run) === char) run++;
  if (run - start < 7) return undefined;
  if (char === EQ) return run === last ? "separator" : undefined;
  // A label may follow, after a space: `<<<<<<< HEAD`.
  if (run !== last && !/\s/.test(text[run]!)) return undefined;
  return char === LT ? "start" : "end";
}

const CR = "\r".charCodeAt(0);
