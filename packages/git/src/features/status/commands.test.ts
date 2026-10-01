import { rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { beforeAll, describe, expect, it } from "vitest";

import type { Repo } from "../../core/repo";
import { createHistoryRepo, createRepo, git, repos } from "../../test/fixtures";
import { parseDiff } from "../diff/parse";
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
    expect(status.files).toEqual([
      { path: "a file.txt", origPath: null, staged: null, unstaged: "modified" },
      { path: "new file.txt", origPath: null, staged: null, unstaged: "untracked" },
    ]);
  });

  it("diffs the working tree against the index", async () => {
    expect((await getStatus(repo)).changes).toEqual({
      staged: [],
      unstaged: [
        { path: "a file.txt", status: "modified", origPath: null, additions: 1, deletions: 2 },
        {
          path: "new file.txt",
          status: "untracked",
          origPath: null,
          additions: null,
          deletions: null,
        },
      ],
    });
  });
});

describe("a repository without commits", () => {
  it("has an empty status", async () => {
    createRepo("empty");
    const status = await getStatus(await repos.open("empty"));
    expect(status).toMatchObject({ head: { kind: "unborn", name: "main" }, files: [] });
    expect(status.changes).toEqual({ staged: [], unstaged: [] });
  });
});

/** What the changed files were before they came from the status: full diffs and ls-files. */
async function fullDiffs(repo: Repo) {
  const [staged, unstaged, untracked] = await Promise.all([
    repo.read(["diff", "--cached", "--raw", "--numstat", "-z", "-M"]),
    repo.read(["diff", "--raw", "--numstat", "-z"]),
    repo.read(["ls-files", "--others", "--exclude-standard", "-z"]),
  ]);
  return {
    staged: parseDiff(staged),
    unstaged: [
      ...parseDiff(unstaged),
      ...untracked
        .split("\0")
        .filter(Boolean)
        .map((path) => ({
          path,
          status: "untracked",
          origPath: null,
          additions: null,
          deletions: null,
        })),
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

  it("are the same as full diffs with a merge conflict", async () => {
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

    const { changes } = await getStatus(repo);
    expect(changes).toEqual(await fullDiffs(repo));
    expect(changes.unstaged).toEqual([expect.objectContaining({ path: "file.txt" })]);
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
