import { describe, expect, it } from "vitest";

import { parsed } from "./viewer-runtime";

/** A patch adding `lines` lines, each `width` characters wide, as the file `name`. */
function patchOf(name: string, lines: number, width = 10): string {
  const line = "x".repeat(width);
  return [
    `diff --git a/${name} b/${name}`,
    "new file mode 100644",
    "index 0000000000000000000000000000000000000000..4cb29ea38f70d7c61b2a3a25b02e3bdf44905402",
    "--- /dev/null",
    `+++ b/${name}`,
    `@@ -0,0 +1,${lines} @@`,
    ...Array.from({ length: lines }, () => `+${line}`),
    "",
  ].join("\n");
}

describe("parsed", () => {
  it("keeps a patch parsed lately, so it isn't parsed again", () => {
    const patch = patchOf("kept.txt", 3);
    expect(parsed(patch, "kept")).toBe(parsed(patch, "kept"));
  });

  it("parses a patch again once it changed", () => {
    const first = parsed(patchOf("changed.txt", 3), "changed");
    expect(parsed(patchOf("changed.txt", 4), "changed")).not.toBe(first);
  });

  it("lets go of a long patch once a newer one comes, but not of the latest", () => {
    // Longer than all the patches kept together may be.
    const long = patchOf("long.txt", 50_000, 100);
    const short = patchOf("short.txt", 3);
    const kept = parsed(long, "long");
    expect(parsed(long, "long")).toBe(kept);

    const shortDiff = parsed(short, "short");
    expect(parsed(short, "short")).toBe(shortDiff);
    expect(parsed(long, "long")).not.toBe(kept);
  });

  it("keeps the latest sixteen short patches", () => {
    const patches = Array.from({ length: 17 }, (_, i) => patchOf(`file-${i}.txt`, 3));
    const diffs = patches.map((patch, i) => parsed(patch, `file-${i}`));
    expect(parsed(patches[1]!, "file-1")).toBe(diffs[1]);
    expect(parsed(patches[0]!, "file-0")).not.toBe(diffs[0]);
  });
});
