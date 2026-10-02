import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import { NoUpstreamError, PullConflictError } from "../../core/errors";
import { cloneRepo, createRepo, git, rejection, repos } from "../../test/fixtures";
import { pull } from "./commands";

/** Writes `content` to `file` in `path` and commits it with `message`. */
function commit(path: string, file: string, content: string, message: string) {
  writeFileSync(join(path, file), content);
  git(path, "add", file);
  git(path, "commit", "-qm", message);
}

/** An upstream repository with one commit, and a clone of it that `repos` opens as `name`. */
function createClone(name: string) {
  const upstream = createRepo(`${name}-upstream`);
  commit(upstream, "a.txt", "a\n", "first");
  const path = cloneRepo(name, upstream);
  return { upstream, path };
}

const subjects = (path: string) => git(path, "log", "--format=%s").split("\n");

describe("pull", () => {
  it("fast-forwards to the upstream's new commits", async () => {
    const { upstream, path } = createClone("behind");
    commit(upstream, "a.txt", "a\nb\n", "second");
    await pull(await repos.open("behind"));
    expect(subjects(path)).toEqual(["second", "first"]);
    expect(readFileSync(join(path, "a.txt"), "utf8")).toBe("a\nb\n");
  });

  it("does nothing when there's nothing new", async () => {
    const { path } = createClone("current");
    await pull(await repos.open("current"));
    expect(subjects(path)).toEqual(["first"]);
  });

  it("merges a branch that diverged from its upstream", async () => {
    const { upstream, path } = createClone("diverged");
    commit(upstream, "theirs.txt", "theirs\n", "theirs");
    commit(path, "ours.txt", "ours\n", "ours");
    await pull(await repos.open("diverged"));
    expect(git(path, "log", "-1", "--format=%P").split(" ")).toHaveLength(2);
    expect(subjects(path)).toEqual(expect.arrayContaining(["theirs", "ours", "first"]));
  });

  it("rebases when the config says to", async () => {
    const { upstream, path } = createClone("rebased");
    git(path, "config", "pull.rebase", "true");
    commit(upstream, "theirs.txt", "theirs\n", "theirs");
    commit(path, "ours.txt", "ours\n", "ours");
    await pull(await repos.open("rebased"));
    expect(subjects(path)).toEqual(["ours", "theirs", "first"]);
  });

  it("leaves conflicts to resolve", async () => {
    const { upstream, path } = createClone("conflicted");
    commit(upstream, "a.txt", "theirs\n", "theirs");
    commit(path, "a.txt", "ours\n", "ours");
    const error = await rejection(pull(await repos.open("conflicted")));
    expect(error).toBeInstanceOf(PullConflictError);
    expect(error).toMatchObject({
      message: "Pulling origin/main caused conflicts. Resolve them, then commit the merge.",
    });
    expect(git(path, "ls-files", "--unmerged")).not.toBe("");

    // Pulling again isn't the conflict's cause; git says why it can't.
    const again = await rejection(pull(await repos.open("conflicted")));
    expect(again).not.toBeInstanceOf(PullConflictError);
    expect(again).toMatchObject({ message: expect.stringContaining("unmerged files") });
  });

  it("says to continue a rebase that stopped at conflicts", async () => {
    const { upstream, path } = createClone("rebase-conflict");
    git(path, "config", "pull.rebase", "true");
    commit(upstream, "a.txt", "theirs\n", "theirs");
    commit(path, "a.txt", "ours\n", "ours");
    await expect(pull(await repos.open("rebase-conflict"))).rejects.toMatchObject({
      message: "Pulling origin/main caused conflicts. Resolve them, then continue the rebase.",
    });
  });

  it("explains why local changes stop it, without git's hints", async () => {
    const { upstream, path } = createClone("dirty");
    commit(upstream, "a.txt", "theirs\n", "theirs");
    writeFileSync(join(path, "a.txt"), "uncommitted\n");
    const error = await rejection(pull(await repos.open("dirty")));
    expect(error).toMatchObject({
      message: expect.stringContaining("Your local changes to the following files would be"),
    });
    expect(error).not.toBeInstanceOf(PullConflictError);
    expect(readFileSync(join(path, "a.txt"), "utf8")).toBe("uncommitted\n");
  });

  it("needs a branch that tracks a remote one", async () => {
    const path = createRepo("local");
    commit(path, "a.txt", "a\n", "first");
    const repo = await repos.open("local");
    await expect(pull(repo)).rejects.toEqual(
      new NoUpstreamError("main doesn't track a remote branch, so there's nothing to pull."),
    );

    git(path, "checkout", "-q", "--detach");
    await expect(pull(repo)).rejects.toEqual(
      new NoUpstreamError("Check out a branch to pull into it."),
    );
  });
});
