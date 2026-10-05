import { hydratePartialDiff, parsePatchFiles, type FileDiffMetadata } from "@pierre/diffs";
import { describe, expect, it } from "vitest";

import { carriedExpansion, fitToPatch, hasMatchingEnds } from "./patch-files";

/** A file of `count` numbered lines, with `changes` made to some of them. */
function lines(count: number, changes: Record<number, string> = {}): string {
  return Array.from({ length: count }, (_, i) => `${changes[i + 1] ?? `line ${i + 1}`}\n`).join("");
}

/** The patch of a change from `before` to `after`, as git would give it, parsed. */
function patchOf(hunks: string): FileDiffMetadata {
  const patch = [
    "diff --git a/f.txt b/f.txt",
    "index 1111111111111111111111111111111111111111..2222222222222222222222222222222222222222 100644",
    "--- a/f.txt",
    "+++ b/f.txt",
    hunks,
  ].join("\n");
  return parsePatchFiles(patch, "test")[0]!.files[0]!;
}

// Line 10 of 40 changed, with 3 lines of context.
const changed10 = patchOf(
  "@@ -7,7 +7,7 @@\n line 7\n line 8\n line 9\n-line 10\n+ten\n line 11\n line 12\n line 13\n",
);

/** `diff` filled in with the whole of a file of 40 lines, and `after`. */
const hydrated = (diff: FileDiffMetadata, after: string) =>
  hydratePartialDiff("clone", diff, {
    oldFile: { name: "f.txt", contents: lines(40) },
    newFile: { name: "f.txt", contents: after },
  });

describe("fitToPatch", () => {
  it("takes the file the patch is of, on either side", () => {
    expect(fitToPatch(changed10, "old", lines(40))).toBe(lines(40));
    expect(fitToPatch(changed10, "new", lines(40, { 10: "ten" }))).toBe(lines(40, { 10: "ten" }));
  });

  it("refuses a file that has changed since", () => {
    expect(() => fitToPatch(changed10, "new", lines(40, { 10: "TEN" }))).toThrow(/has changed/);
    expect(() => fitToPatch(changed10, "new", lines(11, { 10: "ten" }))).toThrow(/has changed/);
  });

  it("takes a file with CRLF line ends for a patch with LF ones, as LF", () => {
    const crlf = lines(40, { 10: "ten" }).replaceAll("\n", "\r\n");
    expect(fitToPatch(changed10, "new", crlf)).toBe(lines(40, { 10: "ten" }));
  });
});

describe("hasMatchingEnds", () => {
  it("tells a file that got lines at its end since its patch was read", () => {
    expect(hasMatchingEnds(hydrated(changed10, lines(40, { 10: "ten" })))).toBe(true);
    expect(hasMatchingEnds(hydrated(changed10, lines(41, { 10: "ten" })))).toBe(false);
  });
});

describe("carriedExpansion", () => {
  it("keeps the lines shown around the changes, by their place in the old file", () => {
    const from = hydrated(changed10, lines(40, { 10: "ten" }));
    // Lines 1-2 above the hunk and 14-18 below it (the lines after the last hunk are its own).
    const expanded = new Map([
      [0, { fromStart: 2, fromEnd: 0 }],
      [1, { fromStart: 5, fromEnd: 0 }],
    ]);
    // Line 30 changed too since: the lines below the first hunk are now split by another.
    const to = hydrated(
      patchOf(
        [
          "@@ -7,7 +7,7 @@\n line 7\n line 8\n line 9\n-line 10\n+ten\n line 11\n line 12\n line 13",
          "@@ -27,7 +27,7 @@\n line 27\n line 28\n line 29\n-line 30\n+thirty\n line 31\n line 32\n line 33\n",
        ].join("\n"),
      ),
      lines(40, { 10: "ten", 30: "thirty" }),
    );
    expect(carriedExpansion(from, expanded, to)).toEqual(
      new Map([
        [0, { fromStart: 2, fromEnd: 0 }],
        [1, { fromStart: 5, fromEnd: 0 }],
      ]),
    );
  });

  it("carries nothing when nothing was shown", () => {
    const from = hydrated(changed10, lines(40, { 10: "ten" }));
    expect(carriedExpansion(from, new Map(), from).size).toBe(0);
  });
});
