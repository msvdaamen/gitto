import { rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { beforeAll, describe, expect, it } from "vitest";

import type { Repo } from "../../core/repo";
import { createHistoryRepo, createRepo, git, repos } from "../../test/fixtures";
import { parseDiff } from "../diff/parse";
import type { FileChange } from "../diff/schema";
import { getStatus } from "./commands";

describe("a repository with history", () => {
  let repo: Repo;

  beforeAll(async () => {
    repo = await createHistoryRepo();
  });

  it("reads the status", async () => {
    const status = await getStatus(repo);
    expect(status).toMatchObject({
      head: { kind: "branch", name: "main" },
      upstream: null,
      ahead: 0,
      behind: 0,
    });
    expect(status.counts).toEqual({ files: 2, staged: 0, unstaged: 2, conflicted: 0 });
  });

  it("lists the changed files, the untracked ones last", async () => {
    expect((await getStatus(repo)).changes).toEqual({
      staged: [],
      unstaged: [
        { path: "a file.txt", status: "modified", origPath: null },
        { path: "new file.txt", status: "untracked", origPath: null },
      ],
    });
  });
});

describe("the version", () => {
  it("stays the same until the status changes", async () => {
    const repo = await createHistoryRepo("versioned");
    const { version } = await getStatus(repo);
    expect((await getStatus(repo)).version).toBe(version);

    // Only the line counts change, which aren't part of the status: git reports the same.
    writeFileSync(join(repo.path, "a file.txt"), "changed\nagain\n");
    expect((await getStatus(repo)).version).toBe(version);

    git(repo.path, "add", "a file.txt");
    expect((await getStatus(repo)).version).not.toBe(version);
  });
});

describe("a repository without commits", () => {
  it("has an empty status", async () => {
    createRepo("empty");
    const status = await getStatus(await repos.open("empty"));
    expect(status).toMatchObject({
      head: { kind: "unborn", name: "main" },
      counts: { files: 0, staged: 0, unstaged: 0, conflicted: 0 },
    });
    expect(status.changes).toEqual({ staged: [], unstaged: [] });
  });
});

/** The files in a raw diff, without line counts. */
function rawChanges(output: string): FileChange[] {
  return parseDiff(output).map(({ path, status, origPath }) => ({ path, status, origPath }));
}

/** The changed files as full diffs and ls-files have them, which the status has to agree with. */
async function fullDiffs(repo: Repo) {
  const [staged, unstaged, untracked] = await Promise.all([
    repo.read(["diff", "--cached", "--raw", "-z", "-M"]),
    repo.read(["diff", "--raw", "-z"]),
    repo.read(["ls-files", "--others", "--exclude-standard", "-z"]),
  ]);
  return {
    staged: rawChanges(staged),
    unstaged: [
      ...rawChanges(unstaged),
      ...untracked
        .split("\0")
        .filter(Boolean)
        .map((path) => ({ path, status: "untracked", origPath: null })),
    ],
  };
}

describe("the changed files", () => {
  it("are the same as full diffs", async () => {
    const path = createRepo("changes");
    // `:colon.txt` would be pathspec magic, if git didn't take paths literally (GIT_LITERAL_PATHSPECS).
    for (const name of ["keep.txt", "edit.txt", "both.txt", "gone.txt", "move.txt", ":colon.txt"]) {
      writeFileSync(join(path, name), `${name}\n`);
    }
    writeFileSync(join(path, "bin.dat"), Buffer.from([0, 1, 2]));
    git(path, "add", ".");
    git(path, "commit", "-q", "-m", "first");

    writeFileSync(join(path, "edit.txt"), "changed\n");
    writeFileSync(join(path, ":colon.txt"), "changed\n");
    writeFileSync(join(path, "both.txt"), "staged\n");
    git(path, "add", "both.txt");
    writeFileSync(join(path, "both.txt"), "staged, then changed again\n");
    rmSync(join(path, "gone.txt"));
    git(path, "mv", "move.txt", "moved.txt");
    writeFileSync(join(path, "bin.dat"), Buffer.from([3, 4, 5]));
    writeFileSync(join(path, "new.txt"), "new\n");
    const repo = await repos.open("changes");

    const { changes } = await getStatus(repo);
    expect(changes).toEqual(await fullDiffs(repo));
    expect(changes.staged).toContainEqual({
      path: "moved.txt",
      status: "renamed",
      origPath: "move.txt",
    });
    expect(changes.unstaged.map((file) => file.path)).toEqual(
      expect.arrayContaining([
        "edit.txt",
        ":colon.txt",
        "both.txt",
        "gone.txt",
        "bin.dat",
        "new.txt",
      ]),
    );
  });

  it("have a merge conflict on both sides", async () => {
    const path = createRepo("conflict");
    writeFileSync(join(path, "file.txt"), "base\n");
    git(path, "add", ".");
    git(path, "commit", "-q", "-m", "base");
    git(path, "checkout", "-q", "-b", "side");
    writeFileSync(join(path, "file.txt"), "side\n");
    git(path, "commit", "-q", "-am", "side");
    git(path, "checkout", "-q", "main");
    writeFileSync(join(path, "file.txt"), "main\n");
    git(path, "commit", "-q", "-am", "main");
    expect(() => git(path, "merge", "-q", "side")).toThrow();
    const repo = await repos.open("conflict");

    const { changes, counts } = await getStatus(repo);
    // The diffs list it on both sides too, but the unstaged one calls it modified.
    const conflict = { path: "file.txt", status: "conflicted", origPath: null };
    expect(changes).toEqual({ staged: [conflict], unstaged: [conflict] });
    expect((await fullDiffs(repo)).staged).toEqual([conflict]);
    expect(counts).toEqual({ files: 1, staged: 0, unstaged: 0, conflicted: 1 });
  });

  it("are the same as full diffs when there are too many paths to list", async () => {
    const path = createRepo("many");
    const names = Array.from({ length: 400 }, (_, i) => `${"long-file-name-".repeat(3)}${i}.txt`);
    for (const name of names) writeFileSync(join(path, name), "a\n");
    git(path, "add", ".");
    git(path, "commit", "-q", "-m", "many");
    for (const name of names) writeFileSync(join(path, name), "b\n");
    const repo = await repos.open("many");

    const { changes } = await getStatus(repo);
    expect(changes.unstaged).toHaveLength(400);
    expect(changes).toEqual(await fullDiffs(repo));
  });
});
