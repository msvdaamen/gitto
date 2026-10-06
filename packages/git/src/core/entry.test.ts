import { mkdirSync, mkdtempSync, readFileSync, symlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import { root } from "../test/fixtures";
import { deleteFiles, entriesAt } from "./entry";

describe("what's in the working tree", () => {
  it("doesn't go through a link where a folder goes, to look or to delete", async () => {
    const tree = mkdtempSync(join(root, "tree-"));
    const outside = mkdtempSync(join(root, "outside-"));
    writeFileSync(join(outside, "file"), "outside\n");
    symlinkSync(outside, join(tree, "dir"));
    mkdirSync(join(tree, "real"));
    writeFileSync(join(tree, "real", "file"), "inside\n");

    expect(await entriesAt(tree, ["dir/file", "real/file", "dir"])).toEqual([
      "blocked",
      "file",
      "link",
    ]);
    const left = await deleteFiles(tree, [Buffer.from("dir/file"), Buffer.from("real/file")]);
    expect(left).toEqual([]);
    expect(readFileSync(join(outside, "file"), "utf8")).toBe("outside\n");
  });
});
