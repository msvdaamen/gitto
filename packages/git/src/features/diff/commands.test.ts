import { writeFileSync } from "node:fs";
import { join } from "node:path";

import { beforeAll, describe, expect, it } from "vitest";

import { GitError } from "../../core/errors";
import type { Repo } from "../../core/repo";
import { createHistoryRepo, createRepo, git, page, rejection, repos } from "../../test/fixtures";
import { getLog } from "../history/commands";
import { getCommitFiles, getLineCounts } from "./commands";

describe("getCommitFiles", () => {
  let repo: Repo;

  beforeAll(async () => {
    repo = await createHistoryRepo();
  });

  it("diffs a merge against its first parent, and a root commit against nothing", async () => {
    const { commits: log } = await getLog(repo, page);
    const merge = log.find((commit) => commit.subject === "Merge side");
    const first = log.find((commit) => commit.subject === "first");
    expect(await getCommitFiles(repo, merge!.sha)).toEqual([
      { path: "c.txt", status: "renamed", origPath: "b.txt", additions: 0, deletions: 0 },
      { path: "s.txt", status: "added", origPath: null, additions: 1, deletions: 0 },
    ]);
    expect(await getCommitFiles(repo, first!.sha)).toEqual([
      { path: "a file.txt", status: "added", origPath: null, additions: 1, deletions: 0 },
      { path: "b.txt", status: "added", origPath: null, additions: 1, deletions: 0 },
      { path: "bin.dat", status: "added", origPath: null, additions: null, deletions: null },
    ]);
  });

  it("explains why a command failed", async () => {
    const error = await rejection(getCommitFiles(repo, "0123456789abcdef0123456789abcdef01234567"));
    expect(error).toBeInstanceOf(GitError);
    expect(error).toMatchObject({ message: expect.stringContaining("bad object") });
  });
});

/** A file asked about that wasn't renamed. */
const file = (path: string) => ({ path, origPath: null });

describe("getLineCounts", () => {
  it("counts the lines of the files asked for, on one side of the index", async () => {
    const path = createRepo("lines");
    for (const name of ["edit.txt", "both.txt", "move.txt", ":colon.txt", "other.txt"]) {
      writeFileSync(join(path, name), `${name}\nsecond line\n`);
    }
    writeFileSync(join(path, "bin.dat"), Buffer.from([0, 1, 2]));
    git(path, "add", ".");
    git(path, "commit", "-q", "-m", "first");

    writeFileSync(join(path, "edit.txt"), "changed\n");
    // `:colon.txt` would be pathspec magic, if git didn't take paths literally.
    writeFileSync(join(path, ":colon.txt"), ":colon.txt\nsecond line\nthird\n");
    writeFileSync(join(path, "other.txt"), "not asked for\n");
    writeFileSync(join(path, "both.txt"), "staged\n");
    git(path, "add", "both.txt");
    writeFileSync(join(path, "both.txt"), "staged\nthen changed again\n");
    git(path, "mv", "move.txt", "moved.txt");
    writeFileSync(join(path, "bin.dat"), Buffer.from([3, 4, 5]));
    writeFileSync(join(path, "new.txt"), "new\n");
    const repo = await repos.open("lines");

    const asked = ["edit.txt", ":colon.txt", "both.txt", "bin.dat", "new.txt"].map(file);
    expect(await getLineCounts(repo, "unstaged", asked)).toEqual([
      { path: ":colon.txt", additions: 1, deletions: 0 },
      { path: "bin.dat", additions: null, deletions: null },
      { path: "both.txt", additions: 1, deletions: 0 },
      { path: "edit.txt", additions: 1, deletions: 2 },
    ]);
    // A rename is recognised by its previous path.
    const staged = [file("both.txt"), { path: "moved.txt", origPath: "move.txt" }];
    expect(await getLineCounts(repo, "staged", staged)).toEqual([
      { path: "both.txt", additions: 1, deletions: 2 },
      { path: "moved.txt", additions: 0, deletions: 0 },
    ]);
  });

  it("counts staged lines before the first commit, and takes more paths than a command line", async () => {
    const path = createRepo("lines-many");
    const names = Array.from({ length: 400 }, (_, i) => `${"long-file-name-".repeat(3)}${i}.txt`);
    for (const name of names) writeFileSync(join(path, name), "a\n");
    git(path, "add", ".");
    const repo = await repos.open("lines-many");

    const counts = await getLineCounts(repo, "staged", names.map(file));
    expect(counts).toHaveLength(400);
    expect(counts).toContainEqual({ path: names[399], additions: 1, deletions: 0 });
  });

  it("keeps a rename's two paths together when they don't fit on one command line", async () => {
    const path = createRepo("lines-renamed");
    // Long enough that the 16,000 characters of a command line run out halfway through them.
    const names = Array.from({ length: 200 }, (_, i) => `${"long-file-name-".repeat(5)}${i}`);
    for (const name of names) writeFileSync(join(path, name), "a\nb\nc\n");
    git(path, "add", ".");
    git(path, "commit", "-q", "-m", "first");
    for (const name of names) git(path, "mv", name, `${name}.moved`);
    const repo = await repos.open("lines-renamed");

    const renames = names.map((name) => ({ path: `${name}.moved`, origPath: name }));
    const counts = await getLineCounts(repo, "staged", renames);
    expect(counts).toHaveLength(200);
    // Every one a rename without changes, not a new file with all of its lines added.
    expect(counts.every((count) => count.additions === 0 && count.deletions === 0)).toBe(true);
  });
});
