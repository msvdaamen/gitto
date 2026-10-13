import { describe, expect, it } from "vitest";

import { patchVersion } from "./patch-version";

const header = [
  "diff --git a/f.txt b/f.txt",
  "index 1111111111111111111111111111111111111111..2222222222222222222222222222222222222222 100644",
  "--- a/f.txt",
  "+++ b/f.txt",
].join("\n");

describe("patchVersion", () => {
  it("names the same patch the same, and another patch of the file otherwise", () => {
    const patch = `${header}\n@@ -1 +1 @@\n-a\n+b\n`;
    expect(patchVersion(patch)).toBe(patchVersion(`${patch}`));
    expect(patchVersion(patch)).not.toBe(patchVersion(patch.replace("2222", "3333")));
    expect(patchVersion(patch)).not.toBe(patchVersion(`${patch}+c\n`));
    // The same length and header, other lines.
    expect(patchVersion(patch)).not.toBe(patchVersion(`${header}\n@@ -1 +1 @@\n-a\n+c\n`));
  });

  it("names a patch without hunks by all of it", () => {
    expect(patchVersion("Binary files differ\n")).not.toBe(patchVersion("Binary files diffir\n"));
  });
});
