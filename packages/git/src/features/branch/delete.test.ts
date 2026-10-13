import { writeFileSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import { BranchDeletionBlockedError } from "../../core/errors";
import { cloneRepo, createRepo, git, rejection, repos, root } from "../../test/fixtures";
import { deleteBranch, fetchedInto } from "./delete";

/** The full names of the refs under `prefix` in the repository at `path`. */
const refs = (path: string, prefix: string) =>
  git(path, "for-each-ref", "--format=%(refname)", prefix).split("\n").filter(Boolean);

/** Commits `file` with `contents` in the repository at `path`. */
function commit(path: string, file: string, contents: string) {
  writeFileSync(join(path, file), contents);
  git(path, "add", ".");
  git(path, "commit", "-qm", file);
}

/** A repository `name` on `main`, with an `other` branch a commit ahead of it. */
function createBranchedRepo(name: string): string {
  const path = createRepo(name);
  commit(path, "a.txt", "a\n");
  git(path, "switch", "-qc", "other");
  commit(path, "other.txt", "other\n");
  git(path, "switch", "-q", "main");
  return path;
}

/**
 * A clone `name` of a branched repository, with a local `other` tracking `origin/other`; the
 * repository it's cloned from, which plays the remote, and the clone.
 */
function createTrackingClone(name: string): { origin: string; path: string } {
  const origin = createBranchedRepo(`${name}-origin`);
  const path = cloneRepo(name, origin);
  git(path, "branch", "-q", "--track", "other", "origin/other");
  return { origin, path };
}

describe("deleting a branch", () => {
  it("deletes a local branch here only, keeping the remote one", async () => {
    const { origin, path } = createTrackingClone("delete-local");

    await deleteBranch(await repos.open("delete-local"), "refs/heads/other");
    expect(refs(path, "refs/heads/")).toEqual(["refs/heads/main"]);
    expect(refs(path, "refs/remotes/origin/")).toContain("refs/remotes/origin/other");
    expect(refs(origin, "refs/heads/")).toContain("refs/heads/other");
    expect(git(path, "config", "--get-regexp", String.raw`^branch\.`)).not.toMatch(
      /^branch\.other\./m,
    );
  });

  it("deletes a local branch with commits no other branch has", async () => {
    const path = createBranchedRepo("delete-unmerged");

    await deleteBranch(await repos.open("delete-unmerged"), "refs/heads/other");
    expect(refs(path, "refs/heads/")).toEqual(["refs/heads/main"]);
  });

  it("refuses to delete the local branch checked out, here or in another worktree", async () => {
    const path = createBranchedRepo("delete-checked-out");
    git(path, "worktree", "add", "-q", join(root, "delete-checked-out-linked"), "other");
    const repo = await repos.open("delete-checked-out");

    await expect(deleteBranch(repo, "refs/heads/main")).rejects.toThrow(
      /cannot delete branch 'main' used by worktree/,
    );
    await expect(deleteBranch(repo, "refs/heads/other")).rejects.toThrow(
      /cannot delete branch 'other' used by worktree/,
    );
    expect(refs(path, "refs/heads/")).toEqual(["refs/heads/main", "refs/heads/other"]);
  });

  it("deletes a remote branch on its remote, and the local branches tracking it", async () => {
    const { origin, path } = createTrackingClone("delete-remote");
    git(path, "branch", "-q", "--track", "also-other", "origin/other");
    git(path, "branch", "-q", "--no-track", "unrelated", "origin/other");

    await deleteBranch(await repos.open("delete-remote"), "refs/remotes/origin/other");
    expect(refs(origin, "refs/heads/")).toEqual(["refs/heads/main"]);
    expect(refs(path, "refs/remotes/origin/")).not.toContain("refs/remotes/origin/other");
    expect(refs(path, "refs/heads/")).toEqual(["refs/heads/main", "refs/heads/unrelated"]);
  });

  it("deletes a remote branch with a slash in its remote's name", async () => {
    const origin = createBranchedRepo("delete-slashed-origin");
    const path = createRepo("delete-slashed");
    git(path, "remote", "add", "team/origin", origin);
    git(path, "fetch", "-q", "team/origin");

    await deleteBranch(await repos.open("delete-slashed"), "refs/remotes/team/origin/other");
    expect(refs(origin, "refs/heads/")).toEqual(["refs/heads/main"]);
    // Without the remote's HEAD, which newer git also records on a first fetch.
    expect(refs(path, "refs/remotes/").filter((ref) => !ref.endsWith("/HEAD"))).toEqual([
      "refs/remotes/team/origin/main",
    ]);
  });

  it("deletes the branch on the remote that its fetch settings fetch it from", async () => {
    const origin = createBranchedRepo("delete-mapped-origin");
    git(origin, "branch", "team/other", "other");
    const path = createRepo("delete-mapped");
    git(path, "remote", "add", "origin", origin);
    git(path, "config", "remote.origin.fetch", "+refs/heads/team/*:refs/remotes/origin/*");
    git(path, "fetch", "-q", "origin");

    await deleteBranch(await repos.open("delete-mapped"), "refs/remotes/origin/other");
    // `team/other`, not `other`, which has the same name here.
    expect(refs(origin, "refs/heads/")).toEqual(["refs/heads/main", "refs/heads/other"]);
    expect(refs(path, "refs/remotes/origin/")).toEqual([]);
  });

  it("deletes nothing when its remote's fetch settings don't fetch it from a branch there", async () => {
    const { origin, path } = createTrackingClone("delete-unmapped");
    git(path, "config", "remote.origin.fetch", "+refs/heads/main:refs/remotes/origin/main");

    const error = await rejection(
      deleteBranch(await repos.open("delete-unmapped"), "refs/remotes/origin/other"),
    );
    expect(error).toBeInstanceOf(BranchDeletionBlockedError);
    expect((error as Error).message).toBe(
      "origin's fetch settings don't fetch origin/other from a branch there, so nothing was deleted.",
    );
    expect(refs(origin, "refs/heads/")).toContain("refs/heads/other");
    expect(refs(path, "refs/heads/")).toContain("refs/heads/other");
  });

  it("deletes nothing when a local branch tracking the remote one is checked out", async () => {
    const { origin, path } = createTrackingClone("delete-remote-checked-out");
    const linked = join(root, "delete-remote-checked-out-linked");
    git(path, "worktree", "add", "-q", linked, "other");

    const error = await rejection(
      deleteBranch(await repos.open("delete-remote-checked-out"), "refs/remotes/origin/other"),
    );
    expect(error).toBeInstanceOf(BranchDeletionBlockedError);
    expect((error as Error).message).toMatch(
      /^other tracks origin\/other and is checked out in .*delete-remote-checked-out-linked, so neither was deleted\. Switch to another branch there first\.$/,
    );
    expect(refs(origin, "refs/heads/")).toContain("refs/heads/other");
    expect(refs(path, "refs/remotes/origin/")).toContain("refs/remotes/origin/other");
    expect(refs(path, "refs/heads/")).toContain("refs/heads/other");
  });

  it("holds up the other writes until it's done, so none checks out a tracking branch meanwhile", async () => {
    const { origin, path } = createTrackingClone("delete-in-turn");
    const repo = await repos.open("delete-in-turn");

    const deleting = deleteBranch(repo, "refs/remotes/origin/other");
    const switching = repo.write(["switch", "other"]);
    await deleting;
    await expect(switching).rejects.toThrow(/invalid reference: other/);
    expect(refs(origin, "refs/heads/")).toEqual(["refs/heads/main"]);
    expect(refs(path, "refs/heads/")).toEqual(["refs/heads/main"]);
  });

  it("deletes nothing when a local branch tracking the remote one is being rebased", async () => {
    const { origin, path } = createTrackingClone("delete-remote-rebasing");
    const linked = join(root, "delete-remote-rebasing-linked");
    git(path, "worktree", "add", "-q", linked, "other");
    commit(path, "main.txt", "main\n");
    // Stops at its first commit, HEAD detached, as an `exec` that fails does.
    expect(() => git(linked, "rebase", "-q", "--exec", "false", "main")).toThrow();

    await expect(
      deleteBranch(await repos.open("delete-remote-rebasing"), "refs/remotes/origin/other"),
    ).rejects.toThrow(
      "other tracks origin/other and is being rebased, so neither was deleted. Finish or abort the rebase first.",
    );
    expect(refs(origin, "refs/heads/")).toContain("refs/heads/other");
    expect(refs(path, "refs/heads/")).toContain("refs/heads/other");
  });

  it("deletes nothing when a local branch tracking the remote one is being bisected", async () => {
    const { origin, path } = createTrackingClone("delete-remote-bisecting");
    git(path, "switch", "-q", "other");
    git(path, "bisect", "start");
    git(path, "switch", "-q", "--detach", "main");

    await expect(
      deleteBranch(await repos.open("delete-remote-bisecting"), "refs/remotes/origin/other"),
    ).rejects.toThrow(
      "other tracks origin/other and is being bisected, so neither was deleted. Stop bisecting first.",
    );
    expect(refs(origin, "refs/heads/")).toContain("refs/heads/other");
    expect(refs(path, "refs/heads/")).toContain("refs/heads/other");
  });

  it("says the remote branch is gone when a local one tracking it couldn't be deleted after", async () => {
    const { origin, path } = createTrackingClone("delete-local-fails");
    // As if another git process were writing the branch.
    writeFileSync(join(path, ".git", "refs", "heads", "other.lock"), "");

    await expect(
      deleteBranch(await repos.open("delete-local-fails"), "refs/remotes/origin/other"),
    ).rejects.toThrow(
      /^origin\/other was deleted on origin, but not every local branch tracking it was\. /,
    );
    expect(refs(origin, "refs/heads/")).toEqual(["refs/heads/main"]);
  });

  it("deletes what's left here of a branch deleted on the remote already", async () => {
    const { origin, path } = createTrackingClone("delete-remote-gone");
    git(origin, "branch", "-qD", "other");

    await deleteBranch(await repos.open("delete-remote-gone"), "refs/remotes/origin/other");
    expect(refs(path, "refs/remotes/origin/")).not.toContain("refs/remotes/origin/other");
    expect(refs(path, "refs/heads/")).toEqual(["refs/heads/main"]);
  });

  it("deletes the branch left here of a remote that's gone", async () => {
    const { path } = createTrackingClone("delete-remote-removed");
    git(path, "update-ref", "refs/remotes/old/other", "origin/other");

    await deleteBranch(await repos.open("delete-remote-removed"), "refs/remotes/old/other");
    expect(refs(path, "refs/remotes/old/")).toEqual([]);
    expect(refs(path, "refs/heads/")).toEqual(["refs/heads/main", "refs/heads/other"]);
  });

  it("says why the remote wouldn't delete it, deleting nothing here", async () => {
    const { path } = createTrackingClone("delete-remote-refused");
    git(path, "remote", "set-url", "origin", join(root, "gone"));

    await expect(
      deleteBranch(await repos.open("delete-remote-refused"), "refs/remotes/origin/other"),
    ).rejects.toThrow(/does not appear to be a git repository/);
    expect(refs(path, "refs/remotes/origin/")).toContain("refs/remotes/origin/other");
    expect(refs(path, "refs/heads/")).toContain("refs/heads/other");
  });

  it("refuses to delete what isn't a branch", async () => {
    const path = createBranchedRepo("delete-other-refs");
    git(path, "tag", "v1");
    const repo = await repos.open("delete-other-refs");

    await expect(deleteBranch(repo, "refs/tags/v1")).rejects.toThrow(
      "refs/tags/v1 isn't a branch.",
    );
    expect(refs(path, "refs/tags/")).toEqual(["refs/tags/v1"]);
  });
});

describe("where a fetch refspec fetches a remote-tracking branch from", () => {
  it("maps it back through the pattern", () => {
    const spec = "+refs/heads/*:refs/remotes/origin/*";
    expect(fetchedInto(spec, "refs/remotes/origin/feature/x")).toBe("refs/heads/feature/x");
    expect(fetchedInto("refs/heads/team/*:refs/remotes/origin/*", "refs/remotes/origin/x")).toBe(
      "refs/heads/team/x",
    );
    expect(
      fetchedInto("refs/heads/*-dev:refs/remotes/origin/*-dev", "refs/remotes/origin/a-dev"),
    ).toBe("refs/heads/a-dev");
  });

  it("maps an exact one", () => {
    expect(
      fetchedInto("refs/heads/main:refs/remotes/origin/main", "refs/remotes/origin/main"),
    ).toBe("refs/heads/main");
    expect(fetchedInto("refs/heads/main:refs/remotes/origin/main", "refs/remotes/origin/x")).toBe(
      undefined,
    );
  });

  it("maps nothing it doesn't fetch into", () => {
    expect(fetchedInto("+refs/heads/*:refs/remotes/origin/*", "refs/remotes/other/x")).toBe(
      undefined,
    );
    expect(fetchedInto("+refs/heads/*:refs/remotes/origin/*", "refs/remotes/origin/")).toBe(
      undefined,
    );
    expect(fetchedInto("^refs/heads/x", "refs/remotes/origin/x")).toBe(undefined);
    expect(fetchedInto("refs/heads/main", "refs/remotes/origin/main")).toBe(undefined);
  });
});
