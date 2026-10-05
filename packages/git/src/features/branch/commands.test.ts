import { chmodSync, mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it, vi } from "vitest";

import { cloneRepo, createRepo, git, repos } from "../../test/fixtures";
import { createBranch, switchBranch } from "./commands";

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

/**
 * A committed repository `name` with an `other` branch that changes `a.txt` and adds `other.txt`,
 * on `main`.
 */
function createBranchedRepo(name: string): string {
  const path = createCommittedRepo(name);
  writeFileSync(join(path, "b.txt"), "b\n");
  git(path, "add", ".");
  git(path, "commit", "-qm", "second");
  git(path, "switch", "-qc", "other");
  writeFileSync(join(path, "a.txt"), "other\na\n");
  writeFileSync(join(path, "other.txt"), "other\n");
  git(path, "add", ".");
  git(path, "commit", "-qm", "other");
  git(path, "switch", "-q", "main");
  return path;
}

const read = (path: string, file: string) => readFileSync(join(path, file), "utf8");

describe("switching branches", () => {
  it("takes the changes along when they're in files the branch doesn't change", async () => {
    const path = createBranchedRepo("switch-along");
    writeFileSync(join(path, "b.txt"), "staged\n");
    git(path, "add", "b.txt");
    writeFileSync(join(path, "new.txt"), "new\n");
    const before = git(path, "status", "--porcelain");

    await switchBranch(await repos.open("switch-along"), "refs/heads/other");
    expect(git(path, "symbolic-ref", "--short", "HEAD")).toBe("other");
    expect(git(path, "status", "--porcelain")).toBe(before);
    expect(git(path, "stash", "list")).toBe("");
  });

  it("refreshes the index a second later, so git doesn't read every file it wrote again", async () => {
    const path = createBranchedRepo("switch-refresh");
    await switchBranch(await repos.open("switch-refresh"), "refs/heads/other");

    // Git compares them in whole seconds: until the index is written in a later second than the
    // files, it can't tell from it whether they changed since.
    const second = (file: string) => Math.floor(statSync(join(path, file)).mtimeMs / 1000);
    await vi.waitFor(() => expect(second(".git/index")).toBeGreaterThan(second("other.txt")), {
      timeout: 3000,
    });
  });

  it("brings changes to files the branch changes along, when they merge", async () => {
    const path = createBranchedRepo("switch-merge");
    // `other` changes the first line; this the last.
    writeFileSync(join(path, "a.txt"), "a\nmine\n");
    writeFileSync(join(path, "new.txt"), "new\n");

    await switchBranch(await repos.open("switch-merge"), "refs/heads/other");
    expect(git(path, "symbolic-ref", "--short", "HEAD")).toBe("other");
    expect(read(path, "a.txt")).toBe("other\na\nmine\n");
    expect(git(path, "status", "--porcelain")).toBe("M a.txt\n?? new.txt");
    expect(git(path, "stash", "list")).toBe("");
  });

  it("keeps the changes in the stash when they conflict with the branch", async () => {
    const path = createBranchedRepo("switch-conflict");
    writeFileSync(join(path, "a.txt"), "mine\n");
    writeFileSync(join(path, "new.txt"), "new\n");

    await expect(
      switchBranch(await repos.open("switch-conflict"), "refs/heads/other"),
    ).rejects.toThrow(
      "Switched to other, but your uncommitted changes conflict with it, so they're kept in the stash.",
    );
    expect(git(path, "symbolic-ref", "--short", "HEAD")).toBe("other");
    expect(git(path, "status", "--porcelain")).toBe("");
    expect(git(path, "stash", "list")).toBe(
      "stash@{0}: On main: Uncommitted changes when switching to other",
    );
    git(path, "switch", "-q", "main");
    git(path, "stash", "pop", "-q");
    expect(read(path, "a.txt")).toBe("mine\n");
    expect(read(path, "new.txt")).toBe("new\n");
  });

  it("keeps the changes in the stash when an untracked file is in the way", async () => {
    const path = createBranchedRepo("switch-untracked");
    // Would merge, but `other` has an `other.txt` of its own.
    writeFileSync(join(path, "b.txt"), "mine\n");
    writeFileSync(join(path, "other.txt"), "mine\n");

    await expect(
      switchBranch(await repos.open("switch-untracked"), "refs/heads/other"),
    ).rejects.toThrow(/kept in the stash/);
    expect(git(path, "symbolic-ref", "--short", "HEAD")).toBe("other");
    expect(git(path, "status", "--porcelain")).toBe("");
    expect(read(path, "other.txt")).toBe("other\n");
    expect(git(path, "stash", "list")).toContain("Uncommitted changes when switching to other");
  });

  it("leaves the changes and the stashes be when git won't switch", async () => {
    const path = createBranchedRepo("switch-refused");
    writeFileSync(join(path, "a.txt"), "mine\n");
    const repo = await repos.open("switch-refused");

    await expect(switchBranch(repo, "refs/heads/missing")).rejects.toThrow(
      /invalid reference: missing/,
    );
    await expect(switchBranch(repo, "refs/heads/--force")).rejects.toThrow(/invalid reference/);
    expect(git(path, "symbolic-ref", "--short", "HEAD")).toBe("main");
    expect(git(path, "status", "--porcelain")).toBe("M a.txt");
    expect(git(path, "stash", "list")).toBe("");
  });

  it("switches to the local branch tracking a remote one", async () => {
    const origin = createBranchedRepo("switch-tracked-origin");
    const path = cloneRepo("switch-tracked", origin);
    git(path, "switch", "-qc", "mine", "--track", "origin/other");
    git(path, "switch", "-q", "main");
    writeFileSync(join(path, "b.txt"), "mine\n");

    await switchBranch(await repos.open("switch-tracked"), "refs/remotes/origin/other");
    expect(git(path, "symbolic-ref", "--short", "HEAD")).toBe("mine");
    expect(git(path, "branch", "--format=%(refname:short)")).toBe("main\nmine");
    expect(git(path, "status", "--porcelain")).toBe("M b.txt");
  });

  it("makes a local branch tracking a remote one, when none does", async () => {
    const origin = createBranchedRepo("switch-remote-origin");
    const path = cloneRepo("switch-remote", origin);
    writeFileSync(join(path, "a.txt"), "a\nmine\n");

    await switchBranch(await repos.open("switch-remote"), "refs/remotes/origin/other");
    expect(git(path, "symbolic-ref", "--short", "HEAD")).toBe("other");
    expect(git(path, "rev-parse", "--abbrev-ref", "other@{upstream}")).toBe("origin/other");
    // Brought along through the stash, as `other` changes `a.txt`.
    expect(read(path, "a.txt")).toBe("other\na\nmine\n");
    expect(git(path, "stash", "list")).toBe("");
  });

  it("names the branch it made when the changes are kept in the stash", async () => {
    const origin = createBranchedRepo("switch-remote-conflict-origin");
    const path = cloneRepo("switch-remote-conflict", origin);
    writeFileSync(join(path, "a.txt"), "mine\n");

    await expect(
      switchBranch(await repos.open("switch-remote-conflict"), "refs/remotes/origin/other"),
    ).rejects.toThrow(
      "Switched to other, but your uncommitted changes conflict with it, so they're kept in the stash.",
    );
    expect(git(path, "symbolic-ref", "--short", "HEAD")).toBe("other");
    expect(git(path, "stash", "list")).toBe(
      "stash@{0}: On main: Uncommitted changes when switching to origin/other",
    );
  });

  it("stashes untracked files in a folder the branch replaces with a file", async () => {
    const path = createCommittedRepo("switch-folder");
    git(path, "switch", "-qc", "file");
    writeFileSync(join(path, "dir"), "file\n");
    git(path, "add", ".");
    git(path, "commit", "-qm", "file");
    git(path, "switch", "-q", "main");
    mkdirSync(join(path, "dir"));
    writeFileSync(join(path, "dir", "tracked.txt"), "tracked\n");
    git(path, "add", ".");
    git(path, "commit", "-qm", "folder");
    git(path, "switch", "-q", "file");
    git(path, "switch", "-q", "main");
    writeFileSync(join(path, "dir", "untracked.txt"), "mine\n");

    // Git won't switch, as that'd lose `dir/untracked.txt`; it's stashed, and kept, as `dir` is a
    // file there.
    await expect(
      switchBranch(await repos.open("switch-folder"), "refs/heads/file"),
    ).rejects.toThrow(/kept in the stash/);
    expect(git(path, "symbolic-ref", "--short", "HEAD")).toBe("file");
    expect(git(path, "stash", "list")).toContain("Uncommitted changes when switching to file");
  });

  it("names the branch it switched to, when a tag has its name too", async () => {
    const path = createBranchedRepo("switch-tag");
    git(path, "tag", "other");
    writeFileSync(join(path, "a.txt"), "mine\n");

    await expect(switchBranch(await repos.open("switch-tag"), "refs/heads/other")).rejects.toThrow(
      /^Switched to other, but/,
    );
  });

  it("brings the changes along when it switched, but a hook failed after", async () => {
    const path = createBranchedRepo("switch-hook");
    const hook = join(path, ".git", "hooks", "post-checkout");
    writeFileSync(hook, "#!/bin/sh\necho hook failed >&2\nexit 1\n");
    chmodSync(hook, 0o755);
    writeFileSync(join(path, "a.txt"), "a\nmine\n");

    await expect(switchBranch(await repos.open("switch-hook"), "refs/heads/other")).rejects.toThrow(
      "hook failed",
    );
    expect(git(path, "symbolic-ref", "--short", "HEAD")).toBe("other");
    expect(read(path, "a.txt")).toBe("other\na\nmine\n");
    expect(git(path, "stash", "list")).toBe("");
  });

  it("names a branch made from a remote one after it without the remote, slashes and all", async () => {
    const origin = createBranchedRepo("switch-slash-origin");
    const path = createRepo("switch-slash");
    git(path, "remote", "add", "up/stream", origin);
    git(path, "fetch", "-q", "up/stream");

    await switchBranch(await repos.open("switch-slash"), "refs/remotes/up/stream/other");
    expect(git(path, "symbolic-ref", "--short", "HEAD")).toBe("other");
    expect(git(path, "rev-parse", "--abbrev-ref", "other@{upstream}")).toBe("up/stream/other");
  });

  it("picks the current branch, then the one named after it, when several track a remote one", async () => {
    const origin = createBranchedRepo("switch-several-origin");
    const path = cloneRepo("switch-several", origin);
    git(path, "branch", "-q", "--track", "a-mine", "origin/other");
    git(path, "branch", "-q", "--track", "other", "origin/other");
    const repo = await repos.open("switch-several");

    await switchBranch(repo, "refs/remotes/origin/other");
    expect(git(path, "symbolic-ref", "--short", "HEAD")).toBe("other");
    git(path, "switch", "-q", "a-mine");
    await switchBranch(repo, "refs/remotes/origin/other");
    expect(git(path, "symbolic-ref", "--short", "HEAD")).toBe("a-mine");
  });
});

describe("creating a branch from another", () => {
  it("switches to a new branch at it, taking the changes along", async () => {
    const path = createBranchedRepo("create-from");
    writeFileSync(join(path, "b.txt"), "staged\n");
    git(path, "add", "b.txt");
    writeFileSync(join(path, "new.txt"), "new\n");
    const before = git(path, "status", "--porcelain");

    await createBranch(await repos.open("create-from"), "topic", "refs/heads/other");
    expect(git(path, "symbolic-ref", "--short", "HEAD")).toBe("topic");
    expect(git(path, "rev-parse", "topic")).toBe(git(path, "rev-parse", "other"));
    expect(git(path, "status", "--porcelain")).toBe(before);
  });

  it("brings changes to files it changes along, when they merge", async () => {
    const path = createBranchedRepo("create-from-merge");
    writeFileSync(join(path, "a.txt"), "a\nmine\n");

    await createBranch(await repos.open("create-from-merge"), "topic", "refs/heads/other");
    expect(git(path, "symbolic-ref", "--short", "HEAD")).toBe("topic");
    expect(read(path, "a.txt")).toBe("other\na\nmine\n");
    expect(git(path, "stash", "list")).toBe("");
  });

  it("keeps the changes in the stash when they conflict with it", async () => {
    const path = createBranchedRepo("create-from-conflict");
    writeFileSync(join(path, "a.txt"), "mine\n");

    await expect(
      createBranch(await repos.open("create-from-conflict"), "topic", "refs/heads/other"),
    ).rejects.toThrow(
      "Switched to topic, but your uncommitted changes conflict with it, so they're kept in the stash.",
    );
    expect(git(path, "rev-parse", "topic")).toBe(git(path, "rev-parse", "other"));
    expect(git(path, "status", "--porcelain")).toBe("");
    expect(git(path, "stash", "list")).toBe(
      "stash@{0}: On main: Uncommitted changes when switching to topic",
    );
  });

  it("makes one from a remote branch that doesn't track it", async () => {
    const origin = createBranchedRepo("create-from-remote-origin");
    const path = cloneRepo("create-from-remote", origin);
    git(path, "config", "branch.autoSetupMerge", "always");

    await createBranch(
      await repos.open("create-from-remote"),
      "topic",
      "refs/remotes/origin/other",
    );
    expect(git(path, "symbolic-ref", "--short", "HEAD")).toBe("topic");
    expect(git(path, "rev-parse", "topic")).toBe(git(path, "rev-parse", "origin/other"));
    expect(git(path, "for-each-ref", "--format=%(upstream)", "refs/heads/topic")).toBe("");
  });

  it("leaves everything be when the name is taken", async () => {
    const path = createBranchedRepo("create-from-taken");
    writeFileSync(join(path, "a.txt"), "mine\n");

    await expect(
      createBranch(await repos.open("create-from-taken"), "main", "refs/heads/other"),
    ).rejects.toThrow("fatal: a branch named 'main' already exists");
    expect(git(path, "symbolic-ref", "--short", "HEAD")).toBe("main");
    expect(git(path, "status", "--porcelain")).toBe("M a.txt");
    expect(git(path, "stash", "list")).toBe("");
  });

  it("refuses to make one from what isn't a branch", async () => {
    const path = createBranchedRepo("create-from-other-refs");
    git(path, "tag", "v1");
    const repo = await repos.open("create-from-other-refs");

    for (const from of ["refs/heads/other~1", "refs/heads/missing", "refs/tags/v1"]) {
      // oxlint-disable-next-line no-await-in-loop -- one at a time, on purpose.
      await expect(createBranch(repo, "topic", from)).rejects.toThrow(`${from} isn't a branch.`);
    }
    expect(git(path, "symbolic-ref", "--short", "HEAD")).toBe("main");
    expect(git(path, "branch", "--format=%(refname:short)")).toBe("main\nother");
  });
});
