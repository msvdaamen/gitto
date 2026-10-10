import { existsSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import { RepositoryChangedError, WorktreeRemovalBlockedError } from "../../core/errors";
import { cloneRepo, createRepo, git, paths, repos, root } from "../../test/fixtures";
import { addWorktree, listWorktrees, removeWorktree } from "./commands";

/** A repository with `a.txt` committed on `main`, and a `feature` branch, which `repos` opens as `name`. */
function createCommittedRepo(name: string): string {
  const path = createRepo(name);
  writeFileSync(join(path, "a.txt"), "a\n");
  git(path, "add", ".");
  git(path, "commit", "-qm", "first");
  git(path, "branch", "feature");
  return path;
}

/** The worktrees as `name [branch]`, with `*` for the current one and `!` for a locked one. */
async function summary(name: string): Promise<string[]> {
  return (await listWorktrees(await repos.open(name))).map(
    (worktree) =>
      `${worktree.name}${worktree.current ? "*" : ""}${worktree.locked === null ? "" : "!"} [${
        worktree.branch ?? "detached"
      }]`,
  );
}

describe("listing worktrees", () => {
  it("lists the main worktree alone, as the current one", async () => {
    const path = createCommittedRepo("alone");
    expect(await listWorktrees(await repos.open("alone"))).toEqual([
      {
        path,
        name: "alone",
        head: git(path, "rev-parse", "HEAD"),
        branch: "refs/heads/main",
        main: true,
        bare: false,
        current: true,
        locked: null,
        prunable: null,
      },
    ]);
  });

  it("lists the linked worktrees after it, on a branch or detached, and which is locked", async () => {
    const path = createCommittedRepo("linked");
    git(path, "worktree", "add", "-q", join(root, "linked-feature"), "feature");
    git(path, "worktree", "add", "-q", "--detach", join(root, "linked-check"));
    git(path, "worktree", "lock", join(root, "linked-check"), "--reason", "busy");

    expect(await summary("linked")).toEqual([
      "linked* [refs/heads/main]",
      "linked-check! [detached]",
      "linked-feature [refs/heads/feature]",
    ]);
    const check = (await listWorktrees(await repos.open("linked")))[1]!;
    expect(check.locked).toBe("busy");
    expect(check.head).toBe(git(path, "rev-parse", "HEAD"));
  });

  it("marks the linked worktree current when the repository is opened as it", async () => {
    const path = createCommittedRepo("opened");
    const linked = join(root, "opened-feature");
    git(path, "worktree", "add", "-q", linked, "feature");
    paths.set("opened-feature", linked);

    expect(await summary("opened-feature")).toEqual([
      "opened [refs/heads/main]",
      "opened-feature* [refs/heads/feature]",
    ]);
  });
});

describe("adding a worktree", () => {
  it("checks a local branch out in a new folder, the parents made as needed", async () => {
    createCommittedRepo("add");
    const folder = join(root, "add-worktrees", "feature");

    await addWorktree(await repos.open("add"), folder, "refs/heads/feature");
    expect(git(folder, "symbolic-ref", "HEAD")).toBe("refs/heads/feature");
    expect(existsSync(join(folder, "a.txt"))).toBe(true);
    expect(await summary("add")).toEqual([
      "add* [refs/heads/main]",
      "feature [refs/heads/feature]",
    ]);
  });

  it("takes a folder relative to the repository", async () => {
    createCommittedRepo("relative");

    await addWorktree(await repos.open("relative"), "../relative-feature", "refs/heads/feature");
    expect(git(join(root, "relative-feature"), "symbolic-ref", "HEAD")).toBe("refs/heads/feature");
  });

  it("creates a new branch from a local one there, not tracking it", async () => {
    const path = createCommittedRepo("new-branch");
    const folder = join(root, "new-branch-topic");

    await addWorktree(await repos.open("new-branch"), folder, "refs/heads/main", "topic");
    expect(git(folder, "symbolic-ref", "HEAD")).toBe("refs/heads/topic");
    expect(git(path, "rev-parse", "topic")).toBe(git(path, "rev-parse", "main"));
    expect(git(path, "for-each-ref", "--format=%(upstream)", "refs/heads/topic")).toBe("");
    // The branch is still checked out here.
    expect(git(path, "symbolic-ref", "HEAD")).toBe("refs/heads/main");
  });

  it("creates a branch tracking a remote one there, or uses the one that tracks it", async () => {
    const path = createCommittedRepo("remote");
    const clone = cloneRepo("remote-clone", path);
    const repo = await repos.open("remote-clone");

    await addWorktree(repo, join(root, "remote-clone-feature"), "refs/remotes/origin/feature");
    expect(git(join(root, "remote-clone-feature"), "symbolic-ref", "HEAD")).toBe(
      "refs/heads/feature",
    );
    expect(git(clone, "for-each-ref", "--format=%(upstream)", "refs/heads/feature")).toBe(
      "refs/remotes/origin/feature",
    );

    // `feature` tracks it now, and is checked out in that worktree.
    await expect(
      addWorktree(repo, join(root, "remote-clone-again"), "refs/remotes/origin/feature"),
    ).rejects.toThrow(/'feature' is already used by worktree at/);

    await addWorktree(
      repo,
      join(root, "remote-clone-topic"),
      "refs/remotes/origin/feature",
      "topic",
    );
    expect(git(clone, "for-each-ref", "--format=%(upstream)", "refs/heads/topic")).toBe(
      "refs/remotes/origin/feature",
    );
  });

  it("refuses a branch that's checked out already, and a folder that's in use", async () => {
    const path = createCommittedRepo("refused");
    const repo = await repos.open("refused");

    await expect(addWorktree(repo, join(root, "refused-main"), "refs/heads/main")).rejects.toThrow(
      `fatal: 'main' is already used by worktree at '${path}'`,
    );
    await expect(addWorktree(repo, path, "refs/heads/feature")).rejects.toThrow(
      `fatal: '${path}' already exists`,
    );
    await expect(addWorktree(repo, join(root, "refused-x"), "refs/heads/gone")).rejects.toThrow(
      "refs/heads/gone isn't a branch.",
    );
    await expect(
      addWorktree(repo, join(root, "refused-x"), "refs/heads/feature", "a..b"),
    ).rejects.toThrow(/^fatal: 'a\.\.b' is not a valid branch name$/);
  });
});

describe("removing a worktree", () => {
  it("deletes the worktree's folder, uncommitted changes and all, keeping its branch", async () => {
    const path = createCommittedRepo("remove");
    const folder = join(root, "remove-feature");
    git(path, "worktree", "add", "-q", folder, "feature");
    writeFileSync(join(folder, "a.txt"), "changed\n");
    writeFileSync(join(folder, "new.txt"), "new\n");

    await removeWorktree(await repos.open("remove"), folder);
    expect(existsSync(folder)).toBe(false);
    expect(await summary("remove")).toEqual(["remove* [refs/heads/main]"]);
    expect(git(path, "rev-parse", "--verify", "refs/heads/feature")).toBeTruthy();
  });

  it("removes one whose folder is gone already", async () => {
    const path = createCommittedRepo("pruned");
    const folder = join(root, "pruned-feature");
    git(path, "worktree", "add", "-q", folder, "feature");
    // Deleted outside git.
    rmSync(folder, { recursive: true });
    expect((await listWorktrees(await repos.open("pruned")))[1]?.prunable).not.toBeNull();

    await removeWorktree(await repos.open("pruned"), folder);
    expect(await summary("pruned")).toEqual(["pruned* [refs/heads/main]"]);
  });

  it("refuses the main worktree, the one open, and one that's gone", async () => {
    const path = createCommittedRepo("blocked");
    const linked = join(root, "blocked-feature");
    git(path, "worktree", "add", "-q", linked, "feature");
    paths.set("blocked-feature", linked);

    await expect(removeWorktree(await repos.open("blocked"), path)).rejects.toThrow(
      WorktreeRemovalBlockedError,
    );
    await expect(removeWorktree(await repos.open("blocked-feature"), linked)).rejects.toThrow(
      WorktreeRemovalBlockedError,
    );
    await expect(
      removeWorktree(await repos.open("blocked"), join(root, "blocked-gone")),
    ).rejects.toThrow(RepositoryChangedError);
    expect(existsSync(linked)).toBe(true);
  });

  it("refuses a locked one, saying so", async () => {
    const path = createCommittedRepo("locked");
    const linked = join(root, "locked-feature");
    git(path, "worktree", "add", "-q", linked, "feature");
    git(path, "worktree", "lock", linked, "--reason", "busy");

    await expect(removeWorktree(await repos.open("locked"), linked)).rejects.toThrow(
      /lock reason: busy/,
    );
    expect(existsSync(linked)).toBe(true);
  });
});
