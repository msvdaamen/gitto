import { utimesSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import { createRepo, git, repos } from "../../test/fixtures";
import { getStatus } from "./commands";
import { refreshStaleIndex } from "./refresh";

/** The files git would have to read to know whether they changed: they don't match the index. */
function unmatched(path: string): string[] {
  return git(path, "diff-files", "--name-only").split("\n").filter(Boolean);
}

describe("refreshing a stale index", () => {
  it("makes git remember touched files again, after a slow status", async () => {
    const path = createRepo("stale");
    writeFileSync(join(path, "touched.txt"), "same\n");
    writeFileSync(join(path, "edited.txt"), "before\n");
    git(path, "add", ".");
    git(path, "commit", "-q", "-m", "first");
    // Written again without changing, like a build or a formatter does.
    const later = new Date(Date.now() + 5000);
    utimesSync(join(path, "touched.txt"), later, later);
    writeFileSync(join(path, "edited.txt"), "after, and longer\n");
    const repo = await repos.open("stale");

    // The status doesn't write to the index, so it finds the same out again every time.
    await getStatus(repo);
    expect(unmatched(path)).toEqual(["edited.txt", "touched.txt"]);

    // A quick status isn't worth taking the index lock for.
    expect(await refreshStaleIndex(repo, 20)).toBe(false);
    expect(unmatched(path)).toEqual(["edited.txt", "touched.txt"]);

    expect(await refreshStaleIndex(repo, 2000)).toBe(true);
    expect(unmatched(path)).toEqual(["edited.txt"]);
    expect((await getStatus(repo)).changes.unstaged).toEqual([
      { path: "edited.txt", status: "modified", origPath: null },
    ]);

    // Not again right away, however slow the status.
    expect(await refreshStaleIndex(repo, 2000)).toBe(false);
  });

  it("leaves the index alone while another git process holds it", async () => {
    const path = createRepo("stale-locked");
    writeFileSync(join(path, "file.txt"), "x\n");
    git(path, "add", ".");
    writeFileSync(join(path, ".git", "index.lock"), "");
    const repo = await repos.open("stale-locked");

    expect(await refreshStaleIndex(repo, 2000)).toBe(false);
  });
});
