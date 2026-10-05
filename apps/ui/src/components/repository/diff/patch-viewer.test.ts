import { describe, expect, it } from "vitest";

import { parsePatch } from "./patch-viewer";

describe("parsePatch", () => {
  it("shows a file that changed type as one change", () => {
    // A symbolic link replaced by a file, as `git diff-tree -p` has it.
    const patch = [
      "diff --git a/link b/link",
      "deleted file mode 120000",
      "index 4cbb553f3f4ac2ee7b01ff6c951d6bf583c39c15..0000000000000000000000000000000000000000",
      "--- a/link",
      "+++ /dev/null",
      "@@ -1 +0,0 @@",
      "-target.txt",
      "\\ No newline at end of file",
      "diff --git a/link b/link",
      "new file mode 100644",
      "index 0000000000000000000000000000000000000000..4cb29ea38f70d7c61b2a3a25b02e3bdf44905402",
      "--- /dev/null",
      "+++ b/link",
      "@@ -0,0 +1,3 @@",
      "+one",
      "+two",
      "+three",
      "",
    ].join("\n");

    const diff = parsePatch(patch, "a1:link");
    expect(diff.type).toBe("change");
    expect(diff.deletionLines).toEqual(["target.txt"]);
    expect(diff.additionLines).toEqual(["one\n", "two\n", "three\n"]);
  });
});
