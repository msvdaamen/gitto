import { describe, expect, it } from "vitest";

import { LinesNotStageableError } from "../../core/errors";
import { linesPatch } from "./lines";
import type { LineSelection } from "./schema";

/** A patch from lines, each ending in a newline. */
const patch = (...lines: string[]) => `${lines.join("\n")}\n`;

/** Picks single lines: removed ones by their old number, added ones by their new one. */
const pick = (deletions: number[], additions: number[]): LineSelection => ({
  deletions: deletions.map((line) => ({ start: line, end: line })),
  additions: additions.map((line) => ({ start: line, end: line })),
});

const HEADER = [
  "diff --git a/f.txt b/f.txt",
  "index 1111111..2222222 100644",
  "--- a/f.txt",
  "+++ b/f.txt",
];
const BUILT = ["diff --git a/f.txt b/f.txt", "--- a/f.txt", "+++ b/f.txt"];

// a b c d e → a B C d e x
const CHANGED = patch(
  ...HEADER,
  "@@ -1,5 +1,6 @@ heading",
  " a",
  "-b",
  "-c",
  "+B",
  "+C",
  " d",
  " e",
  "+x",
);

describe("staging lines", () => {
  it("keeps the picked lines, drops added ones and keeps removed ones unchanged", () => {
    expect(linesPatch(CHANGED, pick([2], [3]), "stage")).toBe(
      patch(...BUILT, "@@ -1,5 +1,5 @@ heading", " a", "-b", " c", "+C", " d", " e"),
    );
  });

  it("stages a whole hunk as it is, but for its index line", () => {
    expect(linesPatch(CHANGED, pick([2, 3], [2, 3, 6]), "stage")).toBe(
      patch(...BUILT, "@@ -1,5 +1,6 @@ heading", " a", "-b", "-c", "+B", "+C", " d", " e", "+x"),
    );
  });

  it("stages only removed lines, or only added ones", () => {
    expect(linesPatch(CHANGED, pick([3], []), "stage")).toBe(
      patch(...BUILT, "@@ -1,5 +1,4 @@ heading", " a", " b", "-c", " d", " e"),
    );
    expect(linesPatch(CHANGED, pick([], [6]), "stage")).toBe(
      patch(...BUILT, "@@ -1,5 +1,6 @@ heading", " a", " b", " c", " d", " e", "+x"),
    );
  });

  it("takes ranges, which can overlap and come in any order", () => {
    const selection = {
      deletions: [{ start: 3, end: 3 }],
      additions: [
        { start: 6, end: 6 },
        { start: 2, end: 3 },
        { start: 3, end: 3 },
      ],
    };
    expect(linesPatch(CHANGED, selection, "stage")).toBe(
      patch(...BUILT, "@@ -1,5 +1,7 @@ heading", " a", " b", "-c", "+B", "+C", " d", " e", "+x"),
    );
  });

  it("has nothing to stage without changed lines picked", () => {
    expect(linesPatch(CHANGED, pick([], []), "stage")).toBeUndefined();
    // Unchanged lines, and numbers past the hunk's.
    expect(linesPatch(CHANGED, pick([1, 4, 9], [1, 4, 9]), "stage")).toBeUndefined();
  });

  it("numbers the new side of a hunk after the lines staged before it", () => {
    // Lines 1–20, with 3 changed and 2 added after 15.
    const twoHunks = patch(
      ...HEADER,
      "@@ -1,6 +1,6 @@",
      " 1",
      " 2",
      "-3",
      "+three",
      " 4",
      " 5",
      " 6",
      "@@ -13,6 +13,8 @@",
      " 13",
      " 14",
      " 15",
      "+15a",
      "+15b",
      " 16",
      " 17",
      " 18",
    );
    // The first hunk left out: the second keeps its numbers.
    expect(linesPatch(twoHunks, pick([], [17]), "stage")).toBe(
      patch(...BUILT, "@@ -13,6 +13,7 @@", " 13", " 14", " 15", "+15b", " 16", " 17", " 18"),
    );
    // A line removed in the first: the second's new side starts a line earlier.
    expect(linesPatch(twoHunks, pick([3], [16]), "stage")).toBe(
      patch(
        ...BUILT,
        "@@ -1,6 +1,5 @@",
        " 1",
        " 2",
        "-3",
        " 4",
        " 5",
        " 6",
        "@@ -13,6 +12,7 @@",
        " 13",
        " 14",
        " 15",
        "+15a",
        " 16",
        " 17",
        " 18",
      ),
    );
  });

  it("numbers a hunk without lines on one side by the line before it", () => {
    // A line added at the top of an empty file, and one removed from the end of another.
    const added = patch(...HEADER, "@@ -0,0 +1,2 @@", "+a", "+b");
    expect(linesPatch(added, pick([], [2]), "stage")).toBe(patch(...BUILT, "@@ -0,0 +1 @@", "+b"));
    const removed = patch(...HEADER, "@@ -3,2 +2,0 @@", "-c", "-d");
    expect(linesPatch(removed, pick([4], []), "stage")).toBe(
      patch(...BUILT, "@@ -3,2 +3 @@", " c", "-d"),
    );
  });

  it("keeps a CR at the end of a line", () => {
    const crlf = patch(...HEADER, "@@ -1,2 +1,2 @@", " a\r", "-b\r", "+B\r");
    expect(linesPatch(crlf, pick([2], [2]), "stage")).toBe(
      patch(...BUILT, "@@ -1,2 +1,2 @@", " a\r", "-b\r", "+B\r"),
    );
  });

  it("reads an empty unchanged line without its marker, as `diff.suppressBlankEmpty` has it", () => {
    const blank = patch(...HEADER, "@@ -1,3 +1,3 @@", " a", "", "-c", "+C");
    expect(linesPatch(blank, pick([3], [3]), "stage")).toBe(
      patch(...BUILT, "@@ -1,3 +1,3 @@", " a", " ", "-c", "+C"),
    );
  });

  describe("at the end of a file without a newline", () => {
    // a b → a b x, b without a newline before.
    const appended = patch(
      ...HEADER,
      "@@ -1,2 +1,3 @@",
      " a",
      "-b",
      "\\ No newline at end of file",
      "+b",
      "+x",
    );

    it("removes the old last line and adds it back with a newline, to add lines after it", () => {
      expect(linesPatch(appended, pick([], [3]), "stage")).toBe(
        patch(...BUILT, "@@ -1,2 +1,3 @@", " a", "-b", "\\ No newline at end of file", "+b", "+x"),
      );
    });

    it("keeps it unchanged without a newline when nothing's added after it", () => {
      // b → B, both without a newline: only the removal staged.
      const replaced = patch(
        ...HEADER,
        "@@ -1,2 +1,2 @@",
        " a",
        "-b",
        "\\ No newline at end of file",
        "+B",
        "\\ No newline at end of file",
      );
      expect(linesPatch(replaced, pick([2], []), "stage")).toBe(
        patch(...BUILT, "@@ -1,2 +1 @@", " a", "-b", "\\ No newline at end of file"),
      );
      // Only the addition: b stays, and B is added after it.
      expect(linesPatch(replaced, pick([], [2]), "stage")).toBe(
        patch(
          ...BUILT,
          "@@ -1,2 +1,3 @@",
          " a",
          "-b",
          "\\ No newline at end of file",
          "+b",
          "+B",
          "\\ No newline at end of file",
        ),
      );
    });

    it("keeps the marker of an added last line", () => {
      const noEol = patch(...HEADER, "@@ -1 +1,2 @@", " a", "+b", "\\ No newline at end of file");
      expect(linesPatch(noEol, pick([], [2]), "stage")).toBe(
        patch(...BUILT, "@@ -1 +1,2 @@", " a", "+b", "\\ No newline at end of file"),
      );
    });

    it("unstages a removed last line with a newline when unstaged lines follow it", () => {
      // Staged: a b → a b x, b without a newline in HEAD. Unstaging the removal of b, but not the
      // lines added, puts b back before them: with a newline.
      expect(linesPatch(appended, pick([2], []), "unstage")).toBe(
        patch(...BUILT, "@@ -1,4 +1,3 @@", " a", "-b", " b", " x"),
      );
      // Unstaging x alone leaves b with its newline.
      expect(linesPatch(appended, pick([], [3]), "unstage")).toBe(
        patch(...BUILT, "@@ -1,2 +1,3 @@", " a", " b", "+x"),
      );
    });
  });
});

describe("unstaging lines", () => {
  it("keeps the picked lines, drops removed ones and keeps added ones unchanged", () => {
    expect(linesPatch(CHANGED, pick([2], [3]), "unstage")).toBe(
      patch(...BUILT, "@@ -1,6 +1,6 @@ heading", " a", "-b", " B", "+C", " d", " e", " x"),
    );
  });

  it("numbers the old side of a hunk after the lines unstaged before it", () => {
    const twoHunks = patch(
      ...HEADER,
      "@@ -1,3 +1,4 @@",
      " 1",
      "+1a",
      " 2",
      " 3",
      "@@ -10,3 +11,3 @@",
      " 10",
      "-11",
      "+eleven",
      " 12",
    );
    // The first hunk's line stays staged: the second's old side is the index's, less it.
    expect(linesPatch(twoHunks, pick([11], [12]), "unstage")).toBe(
      patch(...BUILT, "@@ -11,3 +11,3 @@", " 10", "-11", "+eleven", " 12"),
    );
    // Unstaged too: the old side's lines are numbered as without it.
    expect(linesPatch(twoHunks, pick([11], [2, 12]), "unstage")).toBe(
      patch(
        ...BUILT,
        "@@ -1,3 +1,4 @@",
        " 1",
        "+1a",
        " 2",
        " 3",
        "@@ -10,3 +11,3 @@",
        " 10",
        "-11",
        "+eleven",
        " 12",
      ),
    );
  });
});

describe("whole files", () => {
  const ADDED = patch(
    "diff --git a/new.txt b/new.txt",
    "new file mode 100755",
    "index 0000000..2222222",
    "--- /dev/null",
    "+++ b/new.txt",
    "@@ -0,0 +1,3 @@",
    "+a",
    "+b",
    "+c",
  );
  const DELETED = patch(
    "diff --git a/old.txt b/old.txt",
    "deleted file mode 100644",
    "index 1111111..0000000",
    "--- a/old.txt",
    "+++ /dev/null",
    "@@ -1,3 +0,0 @@",
    "-a",
    "-b",
    "-c",
  );

  it("stages some lines of a new file as a new file, with its mode", () => {
    expect(linesPatch(ADDED, pick([], [1, 3]), "stage")).toBe(
      patch(
        "diff --git a/new.txt b/new.txt",
        "new file mode 100755",
        "--- /dev/null",
        "+++ b/new.txt",
        "@@ -0,0 +1,2 @@",
        "+a",
        "+c",
      ),
    );
  });

  it("unstages a new file's lines from it, or the file if they're all of it", () => {
    expect(linesPatch(ADDED, pick([], [2]), "unstage")).toBe(
      patch(
        "diff --git a/new.txt b/new.txt",
        "--- a/new.txt",
        "+++ b/new.txt",
        "@@ -1,2 +1,3 @@",
        " a",
        "+b",
        " c",
      ),
    );
    expect(linesPatch(ADDED, pick([], [1, 2, 3]), "unstage")).toBe(
      patch(
        "diff --git a/new.txt b/new.txt",
        "new file mode 100755",
        "--- /dev/null",
        "+++ b/new.txt",
        "@@ -0,0 +1,3 @@",
        "+a",
        "+b",
        "+c",
      ),
    );
  });

  it("stages a deleted file's lines from it, or its deletion if they're all of it", () => {
    expect(linesPatch(DELETED, pick([2], []), "stage")).toBe(
      patch(
        "diff --git a/old.txt b/old.txt",
        "--- a/old.txt",
        "+++ b/old.txt",
        "@@ -1,3 +1,2 @@",
        " a",
        "-b",
        " c",
      ),
    );
    expect(linesPatch(DELETED, pick([1, 2, 3], []), "stage")).toBe(
      patch(
        "diff --git a/old.txt b/old.txt",
        "deleted file mode 100644",
        "--- a/old.txt",
        "+++ /dev/null",
        "@@ -1,3 +0,0 @@",
        "-a",
        "-b",
        "-c",
      ),
    );
  });

  it("unstages some lines of a deleted file as a deleted file of only those", () => {
    expect(linesPatch(DELETED, pick([1, 3], []), "unstage")).toBe(
      patch(
        "diff --git a/old.txt b/old.txt",
        "deleted file mode 100644",
        "--- a/old.txt",
        "+++ /dev/null",
        "@@ -1,2 +0,0 @@",
        "-a",
        "-c",
      ),
    );
  });

  const RENAMED = patch(
    "diff --git a/old name.txt b/new name.txt",
    "similarity index 80%",
    "rename from old name.txt",
    "rename to new name.txt",
    "index 1111111..2222222 100644",
    "--- a/old name.txt\t",
    "+++ b/new name.txt\t",
    "@@ -1,2 +1,2 @@",
    " a",
    "-b",
    "+B",
  );

  it("stages lines of a rename with the rename", () => {
    expect(linesPatch(RENAMED, pick([], [2]), "stage")).toBe(
      patch(
        "diff --git a/old name.txt b/new name.txt",
        "rename from old name.txt",
        "rename to new name.txt",
        "--- a/old name.txt\t",
        "+++ b/new name.txt\t",
        "@@ -1,2 +1,3 @@",
        " a",
        " b",
        "+B",
      ),
    );
  });

  it("unstages lines of a rename from the renamed file, which stays renamed", () => {
    expect(linesPatch(RENAMED, pick([2], [2]), "unstage")).toBe(
      patch(
        "diff --git a/new name.txt b/new name.txt",
        "--- a/new name.txt\t",
        "+++ b/new name.txt\t",
        "@@ -1,2 +1,2 @@",
        " a",
        "-b",
        "+B",
      ),
    );
  });

  it("names a file as git quoted it", () => {
    const quoted = patch(
      'diff --git "a/tab\\there.txt" "b/tab\\there.txt"',
      "new file mode 100644",
      "--- /dev/null",
      '+++ "b/tab\\there.txt"',
      "@@ -0,0 +1,2 @@",
      "+a",
      "+b",
    );
    expect(linesPatch(quoted, pick([], [1]), "unstage")).toBe(
      patch(
        'diff --git "a/tab\\there.txt" "b/tab\\there.txt"',
        '--- "a/tab\\there.txt"',
        '+++ "b/tab\\there.txt"',
        "@@ -1 +1,2 @@",
        "+a",
        " b",
      ),
    );
  });

  it("leaves a change of mode out", () => {
    const chmod = patch(
      "diff --git a/f.txt b/f.txt",
      "old mode 100644",
      "new mode 100755",
      "index 1111111..2222222",
      "--- a/f.txt",
      "+++ b/f.txt",
      "@@ -1 +1 @@",
      "-a",
      "+A",
    );
    for (const action of ["stage", "unstage"] as const) {
      expect(linesPatch(chmod, pick([1], [1]), action)).toBe(
        patch(...BUILT, "@@ -1 +1 @@", "-a", "+A"),
      );
    }
  });
});

/** Expects `input` to be rejected with `message`, both to stage and unstage, as VERB says. */
function rejects(input: string, message: string) {
  for (const action of ["stage", "unstage"] as const) {
    const verb = `${action}d`;
    expect(() => linesPatch(input, pick([1], [1]), action)).toThrow(
      new LinesNotStageableError(message.replace("VERB", verb)),
    );
  }
}

describe("what can't be staged by line", () => {
  it("rejects a binary file", () => {
    rejects(
      patch(...HEADER.slice(0, 2), "Binary files a/f.txt and b/f.txt differ"),
      "A binary file's changes can only be VERB whole.",
    );
    rejects(
      patch(...HEADER.slice(0, 2), "GIT binary patch", "literal 3", "Hc$@<O00001"),
      "A binary file's changes can only be VERB whole.",
    );
  });

  it("rejects a link and a submodule", () => {
    rejects(
      patch(
        "diff --git a/l b/l",
        "index 1111111..2222222 120000",
        "--- a/l",
        "+++ b/l",
        "@@ -1 +1 @@",
        "-x",
        "\\ No newline at end of file",
        "+y",
        "\\ No newline at end of file",
      ),
      "A link can only be VERB whole.",
    );
    rejects(
      patch(
        "diff --git a/m b/m",
        "index 1111111..2222222 160000",
        "--- a/m",
        "+++ b/m",
        "@@ -1 +1 @@",
        "-Subproject commit 1111111111111111111111111111111111111111",
        "+Subproject commit 2222222222222222222222222222222222222222",
      ),
      "A submodule can only be VERB whole.",
    );
  });

  it("rejects a file that changed type", () => {
    rejects(
      patch(
        "diff --git a/f b/f",
        "deleted file mode 100644",
        "--- a/f",
        "+++ /dev/null",
        "@@ -1 +0,0 @@",
        "-a",
        "diff --git a/f b/f",
        "new file mode 120000",
        "--- /dev/null",
        "+++ b/f",
        "@@ -0,0 +1 @@",
        "+target",
        "\\ No newline at end of file",
      ),
      "A file that changed type can only be VERB whole.",
    );
  });

  it("rejects a patch without changed lines", () => {
    rejects(
      patch("diff --git a/f b/f", "old mode 100644", "new mode 100755"),
      "This file has no changed lines to be VERB.",
    );
    rejects("", "This file has no changed lines to be VERB.");
  });
});
