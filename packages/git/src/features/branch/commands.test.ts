import { writeFileSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import { cloneRepo, createRepo, git, repos } from "../../test/fixtures";
import { createBranch } from "./commands";

/** A repository with `a.txt` committed on `main`, which `repos` opens as `name`. */
function createCommittedRepo(name: string): string {
  const path = createRepo(name);
  writeFileSync(join(path, "a.txt"), "a\n");
  git(path, "add", ".");
  git(path, "commit", "-qm", "first");
  return path;
}

describe("creating a branch", () => {
  it("switches to a new branch at HEAD, taking the uncommitted changes along", async () => {
    const path = createCommittedRepo("changes");
    writeFileSync(join(path, "a.txt"), "staged\n");
    git(path, "add", "a.txt");
    writeFileSync(join(path, "a.txt"), "unstaged\n");
    writeFileSync(join(path, "new.txt"), "new\n");
    const before = git(path, "status", "--porcelain");

    await createBranch(await repos.open("changes"), "feature/new");
    expect(git(path, "symbolic-ref", "--short", "HEAD")).toBe("feature/new");
    expect(git(path, "rev-parse", "feature/new")).toBe(git(path, "rev-parse", "main"));
    expect(git(path, "status", "--porcelain")).toBe(before);
  });

  it("doesn't track the branch it was made from", async () => {
    const path = createCommittedRepo("upstream");
    const clone = cloneRepo("upstream-clone", path);
    // Else a branch made from `main` would track `origin/main` too.
    git(clone, "config", "branch.autoSetupMerge", "inherit");

    await createBranch(await repos.open("upstream-clone"), "topic");
    expect(git(clone, "for-each-ref", "--format=%(upstream)", "refs/heads/topic")).toBe("");
  });

  it("works before the first commit", async () => {
    const path = createRepo("unborn");
    writeFileSync(join(path, "a.txt"), "a\n");

    await createBranch(await repos.open("unborn"), "other");
    expect(git(path, "symbolic-ref", "--short", "HEAD")).toBe("other");
    expect(git(path, "status", "--porcelain")).toBe("?? a.txt");
  });

  it("says why a name can't be used", async () => {
    createCommittedRepo("names");
    const repo = await repos.open("names");

    await expect(createBranch(repo, "main")).rejects.toThrow(
      "fatal: a branch named 'main' already exists",
    );
    await expect(createBranch(repo, "a..b")).rejects.toThrow(
      /^fatal: 'a\.\.b' is not a valid branch name$/,
    );
    await expect(createBranch(repo, "--force")).rejects.toThrow(/not a valid branch name/);
  });
});
