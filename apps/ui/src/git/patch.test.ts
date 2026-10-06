import { describe, expect, it } from "vitest";

import { hasHunks, isLink, patchVersion, stagedWhole, summarizePatch } from "./patch";

const header = [
  "diff --git a/a.txt b/a.txt",
  "index 1111111111111111111111111111111111111111..2222222222222222222222222222222222222222 100644",
  "--- a/a.txt",
  "+++ b/a.txt",
].join("\n");

describe("summarizePatch", () => {
  it("counts the lines added and removed in the hunks, not the header's", () => {
    const patch = `${header}\n@@ -1,3 +1,3 @@\n a\n-b\n+B\n+C\n c\n\\ No newline at end of file\n`;
    expect(summarizePatch(patch)).toEqual({ binary: false, additions: 2, deletions: 1 });
  });

  it("tells a binary file from one without changed lines", () => {
    const binary =
      "diff --git a/x.png b/x.png\nindex 1..2 100644\nBinary files a/x.png and b/x.png differ\n";
    const empty = "diff --git a/e.txt b/e.txt\nnew file mode 100644\nindex 0..e69de29\n";
    expect(summarizePatch(binary)).toEqual({ binary: true, additions: 0, deletions: 0 });
    expect(summarizePatch(empty)).toEqual({ binary: false, additions: 0, deletions: 0 });
  });

  it("counts both halves of a file that changed type, without the second header", () => {
    const patch = [
      "diff --git a/l b/l",
      "deleted file mode 120000",
      "index 1111111..0000000",
      "--- a/l",
      "+++ /dev/null",
      "@@ -1 +0,0 @@",
      "-target",
      "\\ No newline at end of file",
      "diff --git a/l b/l",
      "new file mode 100644",
      "index 0000000..2222222",
      "--- /dev/null",
      "+++ b/l",
      "@@ -0,0 +1,2 @@",
      "+one",
      "+two",
      "",
    ].join("\n");
    expect(summarizePatch(patch)).toEqual({ binary: false, additions: 2, deletions: 1 });
  });

  it("counts a last line without a newline", () => {
    expect(summarizePatch(`${header}\n@@ -0,0 +1 @@\n+only`)).toMatchObject({ additions: 1 });
  });
});

describe("patchVersion", () => {
  it("changes with the object names or the length, and not otherwise", () => {
    const patch = `${header}\n@@ -1 +1 @@\n-a\n+b\n`;
    expect(patchVersion(patch)).toBe(patchVersion(`${patch}`));
    expect(patchVersion(patch)).not.toBe(patchVersion(patch.replace("2222", "3333")));
    expect(patchVersion(patch)).not.toBe(patchVersion(`${patch}+c\n`));
  });
});

describe("hasHunks", () => {
  it("is whether the patch has changed lines, not only a header", () => {
    expect(hasHunks(`${header}\n@@ -1 +1 @@\n-a\n+b\n`)).toBe(true);
    expect(
      hasHunks(`${header.replace("100644", "100755")}\nold mode 100644\nnew mode 100755\n`),
    ).toBe(false);
  });
});

describe("stagedWhole and isLink", () => {
  const link = [
    "diff --git a/l b/l",
    "new file mode 120000",
    "index 0000000..1111111",
    "--- /dev/null",
    "+++ b/l",
    "@@ -0,0 +1 @@",
    "+target",
    "\\ No newline at end of file",
    "",
  ].join("\n");
  const submodule =
    "diff --git a/sub b/sub\nindex 1111111..2222222 160000\n--- a/sub\n+++ b/sub\n@@ -1 +1 @@\n-Subproject commit 1\n+Subproject commit 2\n";
  const typeChange = `${link}diff --git a/l b/l\nnew file mode 100644\nindex 0000000..2222222\n--- /dev/null\n+++ b/l\n@@ -0,0 +1 @@\n+one\n`;

  it("stage a link, a submodule or a file that changed type whole", () => {
    expect(stagedWhole(link)).toBe(true);
    expect(stagedWhole(submodule)).toBe(true);
    expect(stagedWhole(typeChange)).toBe(true);
    expect(stagedWhole(`${header}\n@@ -1 +1 @@\n-a\n+b\n`)).toBe(false);
  });

  it("tell a link by its mode in the header, not in its lines", () => {
    expect(isLink(link)).toBe(true);
    expect(isLink(submodule)).toBe(false);
    expect(isLink(`${header}\n@@ -1 +1 @@\n-a\n+ 120000\n`)).toBe(false);
  });
});
