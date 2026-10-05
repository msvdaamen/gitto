import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import { RepositoryChangedError, StashConflictError } from "../../core/errors";
import type { Repo } from "../../core/repo";
import { createRepo, git, repos } from "../../test/fixtures";
import { parseStatus, STATUS_ARGS } from "../status/parse";
import { getStashFilePatch, getStashFiles, listStashes, popStash, pushStash } from "./commands";

/** Each changed file, with how it changed in the index and the working tree. */
async function statusFiles(repo: Repo) {
  return parseStatus(await repo.read(STATUS_ARGS)).files.map((file) => [
    file.path,
    file.staged,
    file.unstaged,
  ]);
}

/** A repository with `a.txt` and `b.txt` committed, which `repos` opens as `name`. */
function createCommittedRepo(name: string): string {
  const path = createRepo(name);
  writeFileSync(join(path, "a.txt"), "a\n");
  writeFileSync(join(path, "b.txt"), "b\n");
  git(path, "add", ".");
  git(path, "commit", "-qm", "first");
  return path;
}

/** The newest stash's SHA. */
async function newest(repo: Repo): Promise<string> {
  const [stash] = await listStashes(repo);
  if (!stash) return expect.fail("expected a stash");
  return stash.sha;
}

describe("stashes", () => {
  it("lists none before anything is stashed", async () => {
    createCommittedRepo("none");
    expect(await listStashes(await repos.open("none"))).toEqual([]);
  });

  it("stashes every change, and pops them back as they were", async () => {
    const path = createCommittedRepo("round-trip");
    writeFileSync(join(path, "a.txt"), "staged\n");
    git(path, "add", "a.txt");
    writeFileSync(join(path, "b.txt"), "unstaged\n");
    writeFileSync(join(path, "new file.txt"), "new\n");
    const repo = await repos.open("round-trip");
    const before = await statusFiles(repo);

    await pushStash(repo);
    expect(await statusFiles(repo)).toEqual([]);
    const stashes = await listStashes(repo);
    expect(stashes).toEqual([
      {
        sha: git(path, "rev-parse", "refs/stash"),
        base: git(path, "rev-parse", "HEAD"),
        message: expect.stringMatching(/^WIP on main: [0-9a-f]+ first$/),
        createdAt: expect.any(Number),
      },
    ]);
    expect(Math.abs(stashes[0]!.createdAt - Date.now())).toBeLessThan(60_000);
    // What it changed, staged or not, and the untracked files.
    expect(await getStashFiles(repo, stashes[0]!.sha)).toEqual([
      { path: "a.txt", status: "modified", origPath: null, additions: 1, deletions: 1 },
      { path: "b.txt", status: "modified", origPath: null, additions: 1, deletions: 1 },
      { path: "new file.txt", status: "added", origPath: null, additions: 1, deletions: 0 },
    ]);

    await popStash(repo, stashes[0]!.sha);
    // What was staged is staged again.
    expect(await statusFiles(repo)).toEqual(before);
    expect(await listStashes(repo)).toEqual([]);
  });

  it("lists the newest first, and pops only the newest", async () => {
    const path = createCommittedRepo("several");
    writeFileSync(join(path, "a.txt"), "older\n");
    git(path, "stash", "push", "-q", "-m", "older");
    writeFileSync(join(path, "b.txt"), "newer\n");
    git(path, "stash", "push", "-q", "-m", "newer: with a colon");
    const repo = await repos.open("several");

    const stashes = await listStashes(repo);
    expect(stashes.map((stash) => stash.message)).toEqual([
      "On main: newer: with a colon",
      "On main: older",
    ]);

    await expect(popStash(repo, stashes[1]!.sha)).rejects.toBeInstanceOf(RepositoryChangedError);
    expect(await listStashes(repo)).toEqual(stashes);

    await popStash(repo, stashes[0]!.sha);
    expect(await listStashes(repo)).toEqual([stashes[1]]);
    expect(readFileSync(join(path, "b.txt"), "utf8")).toBe("newer\n");
  });

  it("does nothing when there's nothing to stash", async () => {
    createCommittedRepo("clean");
    const repo = await repos.open("clean");
    await pushStash(repo);
    expect(await listStashes(repo)).toEqual([]);
  });

  it("pops without restaging when something is staged already, keeping that staged", async () => {
    const path = createCommittedRepo("staged-already");
    writeFileSync(join(path, "a.txt"), "stashed\n");
    git(path, "add", "a.txt");
    const repo = await repos.open("staged-already");
    await pushStash(repo);
    writeFileSync(join(path, "other.txt"), "other\n");
    git(path, "add", "other.txt");

    await popStash(repo, await newest(repo));
    expect(await statusFiles(repo)).toEqual([
      ["a.txt", null, "modified"],
      ["other.txt", "added", null],
    ]);
    expect(await listStashes(repo)).toEqual([]);
  });

  it("pops without restaging when what was staged no longer applies", async () => {
    const path = createCommittedRepo("index-moved");
    writeFileSync(join(path, "a.txt"), "1\n2\n3\n4\n5\n");
    git(path, "commit", "-qam", "lines");
    writeFileSync(join(path, "a.txt"), "1\nstashed\n3\n4\n5\n");
    git(path, "add", "a.txt");
    writeFileSync(join(path, "new.txt"), "new\n");
    const repo = await repos.open("index-moved");
    await pushStash(repo);
    // Changes a line next to the stashed one: the staged diff no longer applies, but the stash
    // still merges cleanly.
    writeFileSync(join(path, "a.txt"), "1\n2\n3\ncommitted\n5\n");
    git(path, "commit", "-qam", "change a");

    await popStash(repo, await newest(repo));
    expect(await statusFiles(repo)).toEqual([
      ["a.txt", null, "modified"],
      ["new.txt", null, "untracked"],
    ]);
    expect(readFileSync(join(path, "a.txt"), "utf8")).toBe("1\nstashed\n3\ncommitted\n5\n");
    expect(await listStashes(repo)).toEqual([]);
  });

  it("leaves a pop that conflicts to resolve, and keeps the stash", async () => {
    const path = createCommittedRepo("conflict");
    writeFileSync(join(path, "a.txt"), "stashed\n");
    const repo = await repos.open("conflict");
    await pushStash(repo);
    writeFileSync(join(path, "a.txt"), "committed\n");
    git(path, "commit", "-qam", "change a");
    const sha = await newest(repo);

    await expect(popStash(repo, sha)).rejects.toBeInstanceOf(StashConflictError);
    expect(await statusFiles(repo)).toEqual([["a.txt", "conflicted", "conflicted"]]);
    expect(await newest(repo)).toBe(sha);

    // Git refuses to pop over the conflicts, which aren't the pop's.
    const error = await popStash(repo, sha).catch((reason: unknown) => reason);
    expect(error).not.toBeInstanceOf(StashConflictError);
    expect(error).toBeInstanceOf(Error);
  });

  it("says why a pop that would overwrite local changes was refused, and keeps the stash", async () => {
    const path = createCommittedRepo("overwrite");
    writeFileSync(join(path, "a.txt"), "stashed\n");
    const repo = await repos.open("overwrite");
    await pushStash(repo);
    writeFileSync(join(path, "a.txt"), "local\n");
    const sha = await newest(repo);

    const error = await popStash(repo, sha).catch((reason: unknown) => reason);
    expect(error).toBeInstanceOf(Error);
    expect((error as Error).message).toMatch(/local changes .* would be overwritten/);
    expect((error as Error).message).not.toMatch(/Index was not unstashed/);
    expect(readFileSync(join(path, "a.txt"), "utf8")).toBe("local\n");
    expect(await newest(repo)).toBe(sha);
  });

  it("says why changes can't be stashed before the first commit", async () => {
    const path = createRepo("unborn");
    writeFileSync(join(path, "a.txt"), "a\n");
    git(path, "add", "a.txt");
    await expect(pushStash(await repos.open("unborn"))).rejects.toThrow(/initial commit/);
  });
});

describe("getStashFilePatch", () => {
  it("shows a stashed file's changes compared to the stash's base, untracked ones as added", async () => {
    const path = createCommittedRepo("stash-patches");
    const repo = await repos.open("stash-patches");
    writeFileSync(join(path, "a.txt"), "a\nmore\n");
    git(path, "mv", "b.txt", "c.txt");
    writeFileSync(join(path, "new.txt"), "new\n");
    await pushStash(repo);
    const sha = await newest(repo);

    expect(await getStashFilePatch(repo, sha, { path: "a.txt", origPath: null })).toContain(
      "@@ -1 +1,2 @@\n a\n+more\n",
    );
    expect(await getStashFilePatch(repo, sha, { path: "c.txt", origPath: "b.txt" })).toContain(
      "rename from b.txt\nrename to c.txt\n",
    );
    const untracked = await getStashFilePatch(repo, sha, { path: "new.txt", origPath: null });
    expect(untracked).toMatch(/^diff --git a\/new.txt b\/new.txt\nnew file mode 100644\n/);
    expect(untracked).toContain("@@ -0,0 +1 @@\n+new\n");
  });

  it("is empty for a file the stash didn't change, with or without untracked files", async () => {
    const path = createCommittedRepo("stash-unchanged");
    const repo = await repos.open("stash-unchanged");
    writeFileSync(join(path, "a.txt"), "changed\n");
    git(path, "stash", "push", "-q");
    const sha = await newest(repo);
    expect(await getStashFilePatch(repo, sha, { path: "b.txt", origPath: null })).toBe("");
  });
});
