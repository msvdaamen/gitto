import {
  existsSync,
  mkdirSync,
  readFileSync,
  rmSync,
  statSync,
  utimesSync,
  writeFileSync,
} from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import { DiscardBlockedError, RepositoryChangedError } from "../../core/errors";
import type { Repo } from "../../core/repo";
import type { FileStatus } from "../../schema";
import { createDivergedRepo, tryGit } from "../../test/conflicts";
import { createHistoryRepo, createRepo, git, rejection, repos } from "../../test/fixtures";
import { parseStatus, STATUS_ARGS } from "../status/parse";
import { discard, discardAll } from "./discard";

/** Each changed file, with whether its change is staged and unstaged. */
async function statusFiles(repo: Repo) {
  return parseStatus(await repo.read(STATUS_ARGS)).files;
}

/** A file whose changes are discarded, as its list has it. */
const discarded = (
  path: string,
  status: FileStatus = "modified",
  origPath: string | null = null,
) => ({
  path,
  origPath,
  status,
});

describe("discarding a file's changes", () => {
  it("puts a file back as the index has it, keeping what's staged", async () => {
    const path = createRepo("discard-unstaged");
    writeFileSync(join(path, "f.txt"), "one\n");
    git(path, "add", ".");
    git(path, "commit", "-qm", "first");
    writeFileSync(join(path, "f.txt"), "two\n");
    git(path, "add", ".");
    writeFileSync(join(path, "f.txt"), "three\n");
    const repo = await repos.open("discard-unstaged");

    await discard(repo, discarded("f.txt"), "unstaged");
    expect(readFileSync(join(path, "f.txt"), "utf8")).toBe("two\n");
    expect(await statusFiles(repo)).toEqual([
      { path: "f.txt", origPath: null, staged: "modified", unstaged: null },
    ]);
  });

  it("brings back a deleted file, and deletes an untracked one, but no other", async () => {
    const repo = await createHistoryRepo("discard-untracked");
    const path = repo.path;
    rmSync(join(path, "c.txt"));
    // Taken as a glob, its name would match the other untracked file too.
    writeFileSync(join(path, "*.txt"), "star\n");

    await discard(repo, discarded("c.txt", "deleted"), "unstaged");
    await discard(repo, discarded("*.txt", "untracked"), "unstaged");
    expect(readFileSync(join(path, "c.txt"), "utf8")).toBe("b\n");
    expect(existsSync(join(path, "*.txt"))).toBe(false);
    expect(await statusFiles(repo)).toEqual([
      { path: "a file.txt", origPath: null, staged: null, unstaged: "modified" },
      { path: "new file.txt", origPath: null, staged: null, unstaged: "untracked" },
    ]);
  });

  it("puts a file with staged changes back as HEAD has it, unstaged changes and all", async () => {
    const repo = await createHistoryRepo("discard-staged");
    const path = repo.path;
    git(path, "add", "a file.txt");
    writeFileSync(join(path, "a file.txt"), "changed again\n");
    git(path, "add", "new file.txt");
    git(path, "mv", "c.txt", "d.txt");

    await discard(repo, discarded("a file.txt"), "staged");
    await discard(repo, discarded("new file.txt", "added"), "staged");
    await discard(repo, discarded("d.txt", "renamed", "c.txt"), "staged");
    expect(readFileSync(join(path, "a file.txt"), "utf8")).toBe("a\nmore\n");
    // Added, it's deleted, as HEAD doesn't have it; renamed, it's back where it was.
    expect(existsSync(join(path, "new file.txt"))).toBe(false);
    expect(existsSync(join(path, "d.txt"))).toBe(false);
    expect(readFileSync(join(path, "c.txt"), "utf8")).toBe("b\n");
    expect(await statusFiles(repo)).toEqual([]);
  });

  it("deletes a file staged before the first commit", async () => {
    const path = createRepo("discard-unborn");
    writeFileSync(join(path, "x.txt"), "x\n");
    writeFileSync(join(path, "y.txt"), "y\n");
    git(path, "add", ".");
    const repo = await repos.open("discard-unborn");

    await discard(repo, discarded("x.txt", "added"), "staged");
    expect(existsSync(join(path, "x.txt"))).toBe(false);
    expect(await statusFiles(repo)).toEqual([
      { path: "y.txt", origPath: null, staged: "added", unstaged: null },
    ]);
  });

  it("deletes a file that was force-added, though it's ignored", async () => {
    const repo = await createHistoryRepo("discard-ignored");
    const path = repo.path;
    writeFileSync(join(path, ".git", "info", "exclude"), "*.log\n");
    writeFileSync(join(path, "x.log"), "log\n");
    git(path, "add", "-f", "x.log");

    await discard(repo, discarded("x.log", "added"), "staged");
    expect(existsSync(join(path, "x.log"))).toBe(false);
  });

  it("puts back a folder that a file replaced", async () => {
    const path = createRepo("discard-folder");
    mkdirSync(join(path, "foo"));
    writeFileSync(join(path, "foo", "bar.txt"), "bar\n");
    git(path, "add", ".");
    git(path, "commit", "-qm", "first");
    rmSync(join(path, "foo"), { recursive: true });
    writeFileSync(join(path, "foo"), "a file\n");
    git(path, "add", "-A");
    const repo = await repos.open("discard-folder");

    await discard(repo, discarded("foo", "added"), "staged");
    expect(readFileSync(join(path, "foo", "bar.txt"), "utf8")).toBe("bar\n");
    expect(await statusFiles(repo)).toEqual([]);
  });

  it("discards nothing of a submodule, nor of a repository inside this one", async () => {
    const path = createRepo("discard-submodule");
    writeFileSync(join(path, "f.txt"), "f\n");
    git(path, "add", ".");
    git(path, "commit", "-qm", "first");
    const sha = git(path, "rev-parse", "HEAD");
    git(path, "update-index", "--add", "--cacheinfo", `160000,${sha},sub`);
    const repo = await repos.open("discard-submodule");

    expect(await rejection(discard(repo, discarded("sub"), "staged"))).toMatchObject({
      constructor: DiscardBlockedError,
      message:
        "sub is a submodule, so its changes weren't discarded. Discard them in the submodule itself.",
    });
    expect(await rejection(discard(repo, discarded("sub"), "unstaged"))).toBeInstanceOf(
      DiscardBlockedError,
    );
    expect(git(path, "ls-files", "--stage", "sub")).toMatch(/^160000 /);
    expect(
      await rejection(discard(repo, discarded("nested/", "untracked"), "unstaged")),
    ).toBeInstanceOf(DiscardBlockedError);
  });

  it("deletes a file only marked to be added, rather than emptying it", async () => {
    const repo = await createHistoryRepo("discard-intent");
    const path = repo.path;
    writeFileSync(join(path, "n.txt"), "content\n");
    git(path, "add", "-N", "n.txt");

    await discard(repo, discarded("n.txt", "added"), "unstaged");
    expect(existsSync(join(path, "n.txt"))).toBe(false);
    expect(git(path, "ls-files", "n.txt")).toBe("");
  });

  it("deletes a copy, leaving the file it's a copy of as it is", async () => {
    const path = createRepo("discard-copy");
    writeFileSync(join(path, "lib.txt"), "lib\n");
    git(path, "add", ".");
    git(path, "commit", "-qm", "first");
    writeFileSync(join(path, "copy.txt"), "lib\n");
    writeFileSync(join(path, "lib.txt"), "lib, changed\n");
    git(path, "add", ".");
    const repo = await repos.open("discard-copy");

    await discard(repo, discarded("copy.txt", "copied", "lib.txt"), "staged");
    expect(existsSync(join(path, "copy.txt"))).toBe(false);
    expect(readFileSync(join(path, "lib.txt"), "utf8")).toBe("lib, changed\n");
    expect(await statusFiles(repo)).toEqual([
      { path: "lib.txt", origPath: null, staged: "modified", unstaged: null },
    ]);
  });

  it("discards nothing that would lose the files in a folder that replaced it", async () => {
    const path = createRepo("discard-replaced");
    writeFileSync(join(path, "foo"), "a file\n");
    git(path, "add", ".");
    git(path, "commit", "-qm", "first");
    rmSync(join(path, "foo"));
    mkdirSync(join(path, "foo"));
    writeFileSync(join(path, "foo", "x"), "x\n");
    const repo = await repos.open("discard-replaced");

    // Untracked, then staged as added.
    expect(await rejection(discard(repo, discarded("foo", "deleted"), "unstaged"))).toMatchObject({
      constructor: DiscardBlockedError,
      message:
        "foo's changes weren't discarded: putting it back would also lose what's in the folder foo.",
    });
    git(path, "add", "-A");
    expect(await rejection(discard(repo, discarded("foo", "deleted"), "staged"))).toBeInstanceOf(
      DiscardBlockedError,
    );
    expect(readFileSync(join(path, "foo", "x"), "utf8")).toBe("x\n");
    expect(git(path, "ls-files")).toBe("foo/x");
  });

  it("discards nothing of a rename that would lose an untracked file at its old path", async () => {
    const repo = await createHistoryRepo("discard-rename-untracked");
    const path = repo.path;
    git(path, "mv", "c.txt", "d.txt");
    writeFileSync(join(path, "c.txt"), "another file\n");

    expect(
      await rejection(discard(repo, discarded("d.txt", "renamed", "c.txt"), "staged")),
    ).toMatchObject({
      constructor: DiscardBlockedError,
      message:
        "d.txt's changes weren't discarded: putting it back would also lose the untracked file c.txt.",
    });
    expect(readFileSync(join(path, "c.txt"), "utf8")).toBe("another file\n");
    expect(existsSync(join(path, "d.txt"))).toBe(true);
  });

  it("discards nothing of a file that changed since its list was read", async () => {
    const repo = await createHistoryRepo("discard-changed");
    const path = repo.path;
    // Shown with unstaged changes, then untracked in a terminal: discarding would delete it.
    git(path, "rm", "-q", "--cached", "a file.txt");

    expect(await rejection(discard(repo, discarded("a file.txt"), "unstaged"))).toMatchObject({
      constructor: RepositoryChangedError,
      message: "a file.txt changed since it was shown, so its changes weren't discarded.",
    });
    expect(readFileSync(join(path, "a file.txt"), "utf8")).toBe("changed\n");
  });

  it("discards nothing of a folder with a submodule in it in HEAD", async () => {
    const path = createRepo("discard-submodule-below");
    writeFileSync(join(path, "f.txt"), "f\n");
    git(path, "add", ".");
    git(path, "commit", "-qm", "first");
    const sha = git(path, "rev-parse", "HEAD");
    git(path, "update-index", "--add", "--cacheinfo", `160000,${sha},foo/sub`);
    git(path, "commit", "-qm", "submodule");
    git(path, "rm", "-q", "--cached", "foo/sub");
    writeFileSync(join(path, "foo"), "a file\n");
    git(path, "add", "foo");
    const repo = await repos.open("discard-submodule-below");

    expect(await rejection(discard(repo, discarded("foo", "added"), "staged"))).toBeInstanceOf(
      DiscardBlockedError,
    );
    expect(existsSync(join(path, "foo"))).toBe(true);
    expect(git(path, "ls-files")).toBe("f.txt\nfoo");
  });

  it("discards nothing of a conflicted file", async () => {
    const path = createDivergedRepo("discard-conflicted", {
      base: { "f.txt": "base\n" },
      ours: { "f.txt": "ours\n" },
      theirs: { "f.txt": "theirs\n" },
    });
    expect(tryGit(path, "merge", "-q", "side")).not.toBe(0);
    const before = readFileSync(join(path, "f.txt"), "utf8");
    const repo = await repos.open("discard-conflicted");

    expect(
      await rejection(discard(repo, discarded("f.txt", "conflicted"), "staged")),
    ).toBeInstanceOf(DiscardBlockedError);
    expect(
      await rejection(discard(repo, discarded("f.txt", "conflicted"), "unstaged")),
    ).toBeInstanceOf(DiscardBlockedError);
    expect(readFileSync(join(path, "f.txt"), "utf8")).toBe(before);
    expect(git(path, "ls-files", "--unmerged")).not.toBe("");
  });
});

describe("discarding all changes", () => {
  it("puts back what HEAD has and deletes untracked files, keeping ignored ones", async () => {
    const repo = await createHistoryRepo("discard-all");
    const path = repo.path;
    writeFileSync(join(path, ".gitignore"), "*.log\n");
    git(path, "add", ".gitignore");
    git(path, "commit", "-qm", "ignore logs");
    git(path, "add", "a file.txt", "new file.txt");
    writeFileSync(join(path, "a file.txt"), "changed again\n");
    git(path, "mv", "c.txt", "d.txt");
    rmSync(join(path, "s.txt"));
    mkdirSync(join(path, "folder"));
    writeFileSync(join(path, "folder", "untracked.txt"), "u\n");
    writeFileSync(join(path, "debug.log"), "ignored\n");
    // An unchanged file isn't written again, once the index knows it's unchanged.
    const longAgo = new Date("2020-01-01T00:00:00Z");
    utimesSync(join(path, "bin.dat"), longAgo, longAgo);
    git(path, "status");

    await discardAll(repo);
    expect(await statusFiles(repo)).toEqual([]);
    expect(readFileSync(join(path, "a file.txt"), "utf8")).toBe("a\nmore\n");
    expect(readFileSync(join(path, "c.txt"), "utf8")).toBe("b\n");
    expect(readFileSync(join(path, "s.txt"), "utf8")).toBe("s\n");
    for (const gone of ["new file.txt", "d.txt", "folder"]) {
      expect(existsSync(join(path, gone))).toBe(false);
    }
    expect(existsSync(join(path, "debug.log"))).toBe(true);
    expect(statSync(join(path, "bin.dat")).mtime).toEqual(longAgo);
  });

  it("discards nothing while a merge is under way, finished or not", async () => {
    const path = createDivergedRepo("discard-all-merge", {
      base: { "f.txt": "base\n", "g.txt": "base\n" },
      ours: { "f.txt": "ours\n" },
      theirs: { "f.txt": "theirs\n", "g.txt": "theirs\n" },
    });
    expect(tryGit(path, "merge", "-q", "side")).not.toBe(0);
    writeFileSync(join(path, "new.txt"), "new\n");
    const repo = await repos.open("discard-all-merge");

    expect(await rejection(discardAll(repo))).toMatchObject({
      constructor: DiscardBlockedError,
      message: "A merge is under way. Finish or abort it, then discard all changes.",
    });
    // Resolved, its commit would record the merge without its changes.
    writeFileSync(join(path, "f.txt"), "resolved\n");
    git(path, "add", "f.txt");
    expect(await rejection(discardAll(repo))).toBeInstanceOf(DiscardBlockedError);
    expect(existsSync(join(path, "new.txt"))).toBe(true);
    expect(readFileSync(join(path, "f.txt"), "utf8")).toBe("resolved\n");
    expect(readFileSync(join(path, "g.txt"), "utf8")).toBe("theirs\n");
  });

  it("discards nothing while there are conflicts", async () => {
    const path = createRepo("discard-all-conflicts");
    writeFileSync(join(path, "f.txt"), "base\n");
    git(path, "add", ".");
    git(path, "commit", "-qm", "base");
    writeFileSync(join(path, "f.txt"), "stashed\n");
    git(path, "stash", "-q");
    writeFileSync(join(path, "f.txt"), "committed\n");
    git(path, "commit", "-qam", "change");
    // A pop that conflicts leaves no operation under way.
    expect(tryGit(path, "stash", "pop", "-q")).not.toBe(0);
    const repo = await repos.open("discard-all-conflicts");

    expect(await rejection(discardAll(repo))).toMatchObject({
      constructor: DiscardBlockedError,
      message: "Some files have conflicts. Resolve them, then discard all changes.",
    });
    expect(git(path, "ls-files", "--unmerged")).not.toBe("");
  });

  it("discards nothing when there's nothing but submodules' changes", async () => {
    const path = createRepo("discard-all-submodule");
    writeFileSync(join(path, "f.txt"), "f\n");
    git(path, "add", ".");
    git(path, "commit", "-qm", "first");
    const sha = git(path, "rev-parse", "HEAD");
    git(path, "update-index", "--add", "--cacheinfo", `160000,${sha},sub`);
    git(path, "commit", "-qm", "submodule");
    const other = git(path, "commit-tree", "HEAD^{tree}", "-m", "other");
    git(path, "update-index", "--cacheinfo", `160000,${other},sub`);
    const repo = await repos.open("discard-all-submodule");

    expect(await rejection(discardAll(repo))).toMatchObject({
      constructor: DiscardBlockedError,
      message:
        "There's nothing here to discard. A submodule's changes, or those of a repository inside this one, are discarded in it.",
    });
    expect(git(path, "ls-files", "--stage", "sub")).toContain(other);
  });

  it("deletes every file before the first commit", async () => {
    const path = createRepo("discard-all-unborn");
    writeFileSync(join(path, "staged.txt"), "s\n");
    git(path, "add", ".");
    writeFileSync(join(path, "untracked.txt"), "u\n");
    const repo = await repos.open("discard-all-unborn");

    await discardAll(repo);
    expect(await statusFiles(repo)).toEqual([]);
    expect(existsSync(join(path, "staged.txt"))).toBe(false);
    expect(existsSync(join(path, "untracked.txt"))).toBe(false);

    // Nothing's left to discard.
    expect(await rejection(discardAll(repo))).toBeInstanceOf(DiscardBlockedError);
  });
});
