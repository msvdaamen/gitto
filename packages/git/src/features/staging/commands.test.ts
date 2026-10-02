import { existsSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import { IndexLockedError } from "../../core/errors";
import type { Repo } from "../../core/repo";
import { createHistoryRepo, createRepo, git, repos } from "../../test/fixtures";
import { getStatus } from "../status/commands";
import { parseStatus, STATUS_ARGS } from "../status/parse";
import { stage, stageAll, unstage, unstageAll } from "./commands";

/** Each changed file, with whether its change is staged and unstaged. */
async function statusFiles(repo: Repo) {
  return parseStatus(await repo.read(STATUS_ARGS)).files;
}

describe("staging files", () => {
  it("moves staged changes between the working tree and the index", async () => {
    const repo = await createHistoryRepo();
    const change = {
      path: "a file.txt",
      status: "modified",
      origPath: null,
      additions: 1,
      deletions: 2,
    };
    const untrackedFile = {
      path: "new file.txt",
      status: "untracked",
      origPath: null,
      additions: null,
      deletions: null,
    };

    await stage(repo, ["a file.txt"]);
    expect((await getStatus(repo)).changes).toEqual({
      staged: [change],
      unstaged: [untrackedFile],
    });

    await unstage(repo, ["a file.txt"]);
    expect((await getStatus(repo)).changes).toEqual({
      staged: [],
      unstaged: [change, untrackedFile],
    });
  });

  it("stages and unstages before the first commit", async () => {
    const path = createRepo("empty");
    const repo = await repos.open("empty");
    writeFileSync(join(path, "x y.txt"), "hi\n");

    await stage(repo, ["x y.txt"]);
    expect(await statusFiles(repo)).toEqual([
      { path: "x y.txt", origPath: null, staged: "added", unstaged: null },
    ]);
    expect((await getStatus(repo)).changes).toEqual({
      staged: [{ path: "x y.txt", status: "added", origPath: null, additions: 1, deletions: 0 }],
      unstaged: [],
    });

    await unstage(repo, ["x y.txt"]);
    expect(await statusFiles(repo)).toEqual([
      { path: "x y.txt", origPath: null, staged: null, unstaged: "untracked" },
    ]);
  });

  it("recognises another git process holding the index", async () => {
    const path = createRepo("locked");
    const repo = await repos.open("locked");
    writeFileSync(join(path, "file.txt"), "x");
    writeFileSync(join(path, ".git", "index.lock"), "");
    await expect(stage(repo, ["file.txt"])).rejects.toBeInstanceOf(IndexLockedError);
  });
});

describe("a big working tree", () => {
  it("stages and unstages more paths than fit on a Windows command line", async () => {
    const path = createRepo("big-stage");
    const names = Array.from({ length: 1000 }, (_, i) => `${"long-file-name-".repeat(3)}${i}.txt`);
    for (const name of names) writeFileSync(join(path, name), "a\n");
    const repo = await repos.open("big-stage");

    await stage(repo, names);
    expect((await getStatus(repo)).changes.staged).toHaveLength(1000);
    await unstage(repo, names);
    expect((await getStatus(repo)).changes).toMatchObject({ staged: [] });
  });

  it("stages and unstages everything at once", async () => {
    const path = createRepo("all");
    for (const name of ["kept.txt", "deleted.txt", "moved.txt"]) {
      writeFileSync(join(path, name), `${name}\n`);
    }
    git(path, "add", ".");
    git(path, "commit", "-q", "-m", "First");
    writeFileSync(join(path, "kept.txt"), "changed\n");
    rmSync(join(path, "deleted.txt"));
    git(path, "mv", "moved.txt", "renamed.txt");
    writeFileSync(join(path, "new file.txt"), "new\n");
    const repo = await repos.open("all");

    await stageAll(repo);
    expect(
      (await statusFiles(repo)).map((file) => [file.path, file.staged, file.unstaged]),
    ).toEqual([
      ["deleted.txt", "deleted", null],
      ["kept.txt", "modified", null],
      ["new file.txt", "added", null],
      ["renamed.txt", "renamed", null],
    ]);

    await unstageAll(repo);
    expect((await getStatus(repo)).changes.staged).toEqual([]);
    expect((await statusFiles(repo)).map((file) => [file.path, file.unstaged])).toEqual([
      ["deleted.txt", "deleted"],
      ["kept.txt", "modified"],
      ["moved.txt", "deleted"],
      ["new file.txt", "untracked"],
      ["renamed.txt", "untracked"],
    ]);
  });

  it("unstages everything but conflicts, which stay conflicted", async () => {
    const path = createRepo("all-conflict");
    writeFileSync(join(path, "both.txt"), "base\n");
    writeFileSync(join(path, "other.txt"), "other\n");
    git(path, "add", ".");
    git(path, "commit", "-q", "-m", "base");
    git(path, "checkout", "-q", "-b", "side");
    writeFileSync(join(path, "both.txt"), "side\n");
    git(path, "commit", "-q", "-am", "side");
    git(path, "checkout", "-q", "main");
    writeFileSync(join(path, "both.txt"), "main\n");
    git(path, "commit", "-q", "-am", "main");
    expect(() => git(path, "merge", "-q", "side")).toThrow();
    writeFileSync(join(path, "other.txt"), "changed\n");
    git(path, "add", "other.txt");
    const repo = await repos.open("all-conflict");

    await unstageAll(repo);
    expect(await statusFiles(repo)).toEqual([
      { path: "other.txt", origPath: null, staged: null, unstaged: "modified" },
      { path: "both.txt", origPath: null, staged: "conflicted", unstaged: "conflicted" },
    ]);
    expect(existsSync(join(path, ".git", "MERGE_HEAD"))).toBe(true);
  });

  it("stages and unstages everything before the first commit", async () => {
    const path = createRepo("all-empty");
    const repo = await repos.open("all-empty");
    await unstageAll(repo);

    writeFileSync(join(path, "a.txt"), "a\n");
    await stageAll(repo);
    expect((await getStatus(repo)).changes.staged).toHaveLength(1);
    await unstageAll(repo);
    expect(await statusFiles(repo)).toEqual([
      { path: "a.txt", origPath: null, staged: null, unstaged: "untracked" },
    ]);
  });

  it("reports a locked index when staging more paths than git reads before it fails", async () => {
    const path = createRepo("big-locked");
    const repo = await repos.open("big-locked");
    writeFileSync(join(path, ".git", "index.lock"), "");
    // About 1MB: more than the pipe holds, so git exits before taking it all from stdin.
    const names = Array.from({ length: 20_000 }, (_, i) => `${"long-file-name-".repeat(3)}${i}`);
    await expect(stage(repo, names)).rejects.toBeInstanceOf(IndexLockedError);
  });
});
