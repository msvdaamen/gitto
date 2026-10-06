import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import { GitError, NoOperationError } from "../../core/errors";
import type { Repo } from "../../core/repo";
import { commitFiles, createDivergedRepo, createMergeConflict, tryGit } from "../../test/conflicts";
import { createRepo, git, rejection, repos } from "../../test/fixtures";
import { getConflict, markResolved } from "../conflicts/commands";
import { saveWorkingTreeFile } from "../diff/working-tree";
import { popStash } from "../stash/commands";
import { abortOperation, continueOperation, getOperation } from "./commands";

/** Resolves `f.txt`'s conflict as `contents`, and marks it resolved. */
async function resolve(repo: Repo, contents: string): Promise<void> {
  const { text } = await getConflict(repo, "f.txt");
  const version = await saveWorkingTreeFile(repo, "f.txt", contents, {
    version: text!.version,
    overwrite: false,
  });
  await markResolved(repo, "f.txt", version);
}

describe("a merge", () => {
  it("is under way once it stops at conflicts, and continues once they're resolved", async () => {
    const path = createMergeConflict("op-merge");
    const repo = await repos.open("op-merge");
    expect(await getOperation(repo)).toEqual({ kind: "merge", merging: "side", into: "main" });

    // Not with conflicts left: git says why.
    const error = await rejection(continueOperation(repo, "merge"));
    expect(error).toBeInstanceOf(GitError);
    await resolve(repo, "one\nresolved\nthree\n");
    await continueOperation(repo, "merge");

    expect(await getOperation(repo)).toBeNull();
    // With the message git prepared, without the conflicts it listed in a comment.
    expect(git(path, "log", "-1", "--format=%B")).toBe("Merge branch 'side'");
    expect(git(path, "rev-parse", "HEAD^2")).toBe(git(path, "rev-parse", "side"));
  });

  it("is aborted, which puts things back as they were", async () => {
    const path = createMergeConflict("op-merge-abort");
    const repo = await repos.open("op-merge-abort");
    await abortOperation(repo, "merge");

    expect(await getOperation(repo)).toBeNull();
    expect(readFileSync(join(path, "f.txt"), "utf8")).toBe("one\nours\nthree\n");
    expect(git(path, "status", "--porcelain")).toBe("");
  });

  it("names a commit no branch points at by its SHA", async () => {
    const path = createDivergedRepo("op-merge-sha", {
      base: { "f.txt": "one\n" },
      ours: { "f.txt": "ours\n" },
      theirs: { "f.txt": "theirs\n" },
    });
    const side = git(path, "rev-parse", "side");
    git(path, "branch", "-D", "side");
    tryGit(path, "merge", side);
    expect(await getOperation(await repos.open("op-merge-sha"))).toEqual({
      kind: "merge",
      merging: side.slice(0, git(path, "rev-parse", "--short", side).length),
      into: "main",
    });
  });

  it("isn't continued or aborted as another operation", async () => {
    createMergeConflict("op-wrong-kind");
    const repo = await repos.open("op-wrong-kind");
    await expect(abortOperation(repo, "rebase")).rejects.toThrow(
      new NoOperationError("A merge is under way now, not a rebase."),
    );
    await abortOperation(repo, "merge");
    await expect(continueOperation(repo, "merge")).rejects.toThrow(
      new NoOperationError("The merge is no longer under way."),
    );
  });
});

/** A repository whose `side`, with two commits that change `f.txt`'s second line, is rebased onto `main`. */
function createRebase(name: string): string {
  const path = createDivergedRepo(name, {
    base: { "f.txt": "one\ntwo\nthree\n" },
    ours: { "f.txt": "one\nours\nthree\n" },
    theirs: { "f.txt": "one\ntheirs\nthree\n" },
  });
  git(path, "checkout", "-q", "side");
  commitFiles(path, { "f.txt": "one\ntheirs again\nthree\n" }, "theirs again");
  tryGit(path, "rebase", "main");
  return path;
}

describe("a rebase", () => {
  it("says which commit it's at, and stops at the next one's conflicts", async () => {
    const path = createRebase("op-rebase");
    const repo = await repos.open("op-rebase");
    expect(await getOperation(repo)).toEqual({
      kind: "rebase",
      branch: "side",
      onto: "main",
      steps: { step: 1, total: 2 },
    });
    expect((await getConflict(repo, "f.txt")).text?.contents).toMatch(
      /^>>>>>>> [0-9a-f]+ \(theirs\)$/m,
    );

    // The next commit conflicts with how this one was resolved: that's left to resolve too.
    await resolve(repo, "one\nresolved\nthree\n");
    await continueOperation(repo, "rebase");
    expect(await getOperation(repo)).toMatchObject({ steps: { step: 2, total: 2 } });

    await resolve(repo, "one\nresolved again\nthree\n");
    await continueOperation(repo, "rebase");
    expect(await getOperation(repo)).toBeNull();
    expect(git(path, "log", "--format=%s", "main..side")).toBe("theirs again\ntheirs");
  });

  it("is aborted, back on the branch as it was", async () => {
    const path = createRebase("op-rebase-abort");
    const before = git(path, "rev-parse", "ORIG_HEAD");
    const repo = await repos.open("op-rebase-abort");
    await abortOperation(repo, "rebase");

    expect(await getOperation(repo)).toBeNull();
    expect(git(path, "rev-parse", "HEAD")).toBe(before);
    expect(git(path, "branch", "--show-current")).toBe("side");
  });
});

describe("a cherry-pick", () => {
  it("names the commit, and commits it once resolved", async () => {
    const path = createDivergedRepo("op-cherry-pick", {
      base: { "f.txt": "one\ntwo\nthree\n" },
      ours: { "f.txt": "one\nours\nthree\n" },
      theirs: { "f.txt": "one\ntheirs\nthree\n" },
    });
    tryGit(path, "cherry-pick", "side");
    const repo = await repos.open("op-cherry-pick");
    expect(await getOperation(repo)).toEqual({
      kind: "cherry-pick",
      commit: { sha: git(path, "rev-parse", "--short", "side"), subject: "theirs" },
      remaining: 0,
    });

    await resolve(repo, "one\nours and theirs\nthree\n");
    await continueOperation(repo, "cherry-pick");
    expect(await getOperation(repo)).toBeNull();
    expect(git(path, "log", "-1", "--format=%s")).toBe("theirs");
  });

  it("skips a commit whose conflicts were resolved to nothing, which git won't commit", async () => {
    const path = createDivergedRepo("op-cherry-pick-empty", {
      base: { "f.txt": "one\ntwo\nthree\n" },
      ours: { "f.txt": "one\nours\nthree\n" },
      theirs: { "f.txt": "one\ntheirs\nthree\n" },
    });
    const head = git(path, "rev-parse", "HEAD");
    tryGit(path, "cherry-pick", "side");
    const repo = await repos.open("op-cherry-pick-empty");

    await resolve(repo, "one\nours\nthree\n");
    await continueOperation(repo, "cherry-pick");
    expect(await getOperation(repo)).toBeNull();
    expect(git(path, "rev-parse", "HEAD")).toBe(head);
  });

  it("counts the commits left in a series", async () => {
    const path = createDivergedRepo("op-cherry-picks", {
      base: { "f.txt": "one\ntwo\nthree\n" },
      ours: { "f.txt": "one\nours\nthree\n" },
      theirs: { "f.txt": "one\ntheirs\nthree\n" },
    });
    git(path, "checkout", "-q", "side");
    commitFiles(path, { "g.txt": "g\n" }, "another");
    commitFiles(path, { "h.txt": "h\n" }, "and another");
    git(path, "checkout", "-q", "main");
    tryGit(path, "cherry-pick", "main..side");
    expect(await getOperation(await repos.open("op-cherry-picks"))).toMatchObject({
      kind: "cherry-pick",
      commit: { subject: "theirs" },
      remaining: 2,
    });
  });
});

describe("a revert", () => {
  it("names the commit reverted", async () => {
    const path = createRepo("op-revert");
    commitFiles(path, { "f.txt": "one\n" }, "first");
    commitFiles(path, { "f.txt": "two\n" }, "second");
    commitFiles(path, { "f.txt": "three\n" }, "third");
    tryGit(path, "revert", "--no-edit", "HEAD~");
    expect(await getOperation(await repos.open("op-revert"))).toEqual({
      kind: "revert",
      commit: { sha: git(path, "rev-parse", "--short", "HEAD~"), subject: "second" },
      remaining: 0,
    });
  });
});

describe("a stash that conflicts as it's popped", () => {
  it("leaves conflicts to resolve, without an operation", async () => {
    const path = createRepo("op-stash");
    commitFiles(path, { "f.txt": "one\n" }, "first");
    writeFileSync(join(path, "f.txt"), "stashed\n");
    git(path, "stash", "push", "-q");
    commitFiles(path, { "f.txt": "committed\n" }, "second");
    const repo = await repos.open("op-stash");
    await popStash(repo, git(path, "rev-parse", "refs/stash")).catch(() => undefined);

    expect(await getOperation(repo)).toBeNull();
    expect((await getConflict(repo, "f.txt")).text?.contents).toBe(
      "<<<<<<< Updated upstream\ncommitted\n=======\nstashed\n>>>>>>> Stashed changes\n",
    );
  });
});
