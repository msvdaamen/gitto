import { describe, expect, it } from "vitest";

import { PushBlockedError, PushRejectedError } from "../../core/errors";
import {
  cloneRepo,
  createClone,
  createRepo,
  git,
  rejection as rejectionOf,
  repos,
} from "../../test/fixtures";
import { getPushTarget, push, rejection } from "./push";

const subjects = (path: string, rev = "HEAD") => git(path, "log", "--format=%s", rev).split("\n");

const commit = (path: string, message: string) =>
  git(path, "commit", "-q", "--allow-empty", "-m", message);

/** Commits to `upstream` from another clone of it, as someone else pushing. */
function pushFromElsewhere(upstream: string, name: string, message: string) {
  const other = cloneRepo(name, upstream);
  commit(other, message);
  git(other, "push", "-q");
}

describe("push", () => {
  it("pushes the branch to its upstream", async () => {
    const { upstream, path } = createClone("ahead");
    commit(path, "second");
    const repo = await repos.open("ahead");
    expect(await getPushTarget(repo)).toEqual({
      kind: "remote",
      name: "origin/main",
      isNew: false,
      ahead: 1,
      pullsFrom: true,
    });
    await push(repo, { force: false });
    expect(subjects(upstream, "main")).toEqual(["second", "first"]);
    expect(await getPushTarget(repo)).toMatchObject({ ahead: 0 });
  });

  it("pushes a new branch to its remote, and tracks it there", async () => {
    const { upstream, path } = createClone("new-branch");
    git(path, "checkout", "-qb", "feature");
    commit(path, "feature work");
    const repo = await repos.open("new-branch");
    expect(await getPushTarget(repo)).toEqual({
      kind: "remote",
      name: "origin/feature",
      isNew: true,
      ahead: 1,
      pullsFrom: false,
    });
    await push(repo, { force: false });
    expect(subjects(upstream, "feature")).toEqual(["feature work", "first"]);
    expect(git(path, "rev-parse", "--abbrev-ref", "feature@{upstream}")).toBe("origin/feature");
  });

  it("pushes a branch tracking another one under its own name, leaving what it tracks", async () => {
    const { upstream, path } = createClone("other-name");
    git(path, "checkout", "-qb", "feature", "origin/main");
    commit(path, "feature work");
    const repo = await repos.open("other-name");
    expect(await getPushTarget(repo)).toMatchObject({
      name: "origin/feature",
      isNew: true,
      pullsFrom: false,
    });
    await push(repo, { force: false });
    expect(subjects(upstream, "feature")).toEqual(["feature work", "first"]);
    expect(subjects(upstream, "main")).toEqual(["first"]);
    expect(git(path, "rev-parse", "--abbrev-ref", "feature@{upstream}")).toBe("origin/main");

    // Unless the config says to push to the upstream.
    git(path, "config", "push.default", "upstream");
    expect(await getPushTarget(repo)).toMatchObject({
      name: "origin/main",
      ahead: 1,
      pullsFrom: true,
    });
  });

  it("pushes where the config says, e.g. to a fork", async () => {
    const { path } = createClone("triangular");
    const fork = createClone("triangular-fork").upstream;
    git(path, "remote", "add", "fork", fork);
    git(path, "config", "remote.pushDefault", "fork");
    commit(path, "mine");
    const repo = await repos.open("triangular");
    expect(await getPushTarget(repo)).toMatchObject({
      name: "fork/main",
      isNew: true,
      pullsFrom: false,
    });
  });

  it("pushes where the remote's push refspecs say, e.g. for review", async () => {
    const { upstream, path } = createClone("for-review");
    git(path, "config", "remote.origin.push", "refs/heads/*:refs/for/*");
    commit(path, "change");
    const repo = await repos.open("for-review");
    expect(await getPushTarget(repo)).toMatchObject({ name: "origin/refs/for/main" });
    await push(repo, { force: false });
    expect(subjects(upstream, "refs/for/main")).toEqual(["change", "first"]);
    expect(subjects(upstream, "main")).toEqual(["first"]);
  });

  it("pushes to a remote with only a push URL, or to a URL", async () => {
    const { upstream, path } = createClone("by-url");
    // A fork: a copy of the upstream, which takes pushes as it does.
    const fork = cloneRepo("by-url-fork", upstream);
    git(fork, "config", "receive.denyCurrentBranch", "updateInstead");
    git(path, "config", "remote.fork.pushurl", fork);
    git(path, "config", "remote.pushDefault", "fork");
    commit(path, "to the fork");
    const repo = await repos.open("by-url");
    expect(await getPushTarget(repo)).toMatchObject({ kind: "remote", name: "fork/main" });
    await push(repo, { force: false });
    expect(subjects(fork, "main")).toEqual(["to the fork", "first"]);

    git(path, "config", "branch.main.pushRemote", upstream);
    expect(await getPushTarget(repo)).toMatchObject({ kind: "remote", isNew: false });
    await push(repo, { force: false });
    expect(subjects(upstream, "main")).toEqual(["to the fork", "first"]);
  });

  it("finds the remote-tracking branch through the remote's fetch refspecs", async () => {
    const { path } = createClone("refspec");
    git(
      path,
      "config",
      "--replace-all",
      "remote.origin.fetch",
      "+refs/heads/*:refs/remotes/mirror/*",
    );
    git(path, "fetch", "-q", "--prune");
    commit(path, "second");
    expect(await getPushTarget(await repos.open("refspec"))).toMatchObject({
      name: "origin/main",
      isNew: false,
      ahead: 1,
    });
  });

  it("explains a rejection without a remote-tracking branch to compare with", async () => {
    const { upstream, path } = createClone("single");
    // Fetches only main, so the branches it pushes have no remote-tracking branches.
    git(
      path,
      "config",
      "--replace-all",
      "remote.origin.fetch",
      "+refs/heads/main:refs/remotes/origin/main",
    );
    git(path, "checkout", "-qb", "feature");
    commit(path, "feature work");
    const repo = await repos.open("single");
    expect(await getPushTarget(repo)).toMatchObject({ name: "origin/feature", isNew: false });
    await push(repo, { force: false });
    git(path, "commit", "-q", "--amend", "--allow-empty", "-m", "feature work, reworded");
    const error = await rejectionOf(push(repo, { force: false }));
    expect(error).toBeInstanceOf(PushRejectedError);
    expect(error).toMatchObject({ forcible: false });
    expect(subjects(upstream, "feature")).toEqual(["feature work", "first"]);
  });

  it("pushes only the checked-out branch, whatever push.default says", async () => {
    const { upstream, path } = createClone("matching");
    git(path, "config", "push.default", "matching");
    git(path, "branch", "other");
    git(path, "push", "-q", "origin", "other");
    // `other` moves on upstream, so pushing it would be turned down.
    const theirs = git(upstream, "commit-tree", "HEAD^{tree}", "-p", "HEAD", "-m", "theirs");
    git(upstream, "update-ref", "refs/heads/other", theirs);
    git(path, "fetch", "-q");
    commit(path, "ours");
    await push(await repos.open("matching"), { force: false });
    expect(subjects(upstream, "main")).toEqual(["ours", "first"]);
    expect(subjects(upstream, "other")).toEqual(["theirs", "first"]);
  });

  it("offers to force-push an amended commit, and replaces it when forced", async () => {
    const { upstream, path } = createClone("amended");
    commit(path, "second");
    git(path, "push", "-q");
    git(path, "commit", "-q", "--amend", "--allow-empty", "-m", "second, reworded");
    const repo = await repos.open("amended");

    const error = await rejectionOf(push(repo, { force: false }));
    expect(error).toBeInstanceOf(PushRejectedError);
    expect(error).toMatchObject({
      forcible: true,
      message: "origin/main still has main's commits from before a rebase or amend.",
    });
    expect(subjects(upstream, "main")).toEqual(["second", "first"]);

    await push(repo, { force: true });
    expect(subjects(upstream, "main")).toEqual(["second, reworded", "first"]);
  });

  it("doesn't offer to force-push over someone else's commits", async () => {
    const { upstream, path } = createClone("behind");
    pushFromElsewhere(upstream, "behind-other", "theirs");
    commit(path, "ours");
    const repo = await repos.open("behind");
    const notPulled = {
      forcible: false,
      message: "origin/main has commits you haven't pulled yet.",
    };
    // Not fetched yet.
    expect(await rejectionOf(push(repo, { force: false }))).toMatchObject(notPulled);
    // Fetched, but not pulled.
    git(path, "fetch", "-q");
    expect(await rejectionOf(push(repo, { force: false }))).toMatchObject(notPulled);
  });

  it("doesn't force-push over commits pushed since it last pulled", async () => {
    const { upstream, path } = createClone("raced");
    commit(path, "second");
    git(path, "push", "-q");
    git(path, "commit", "-q", "--amend", "--allow-empty", "-m", "second, reworded");
    pushFromElsewhere(upstream, "raced-other", "theirs");
    const repo = await repos.open("raced");

    // Not fetched: the remote changed since.
    const stale = await rejectionOf(push(repo, { force: true }));
    expect(stale).toMatchObject({
      forcible: false,
      message: "origin/main has commits you haven't pulled yet, so it wasn't force-pushed.",
    });
    // Fetched in the background, but never pulled into the branch.
    git(path, "fetch", "-q");
    const unseen = await rejectionOf(push(repo, { force: true }));
    expect(unseen).toMatchObject({
      forcible: false,
      message: expect.stringContaining("wasn't force-pushed"),
    });
    expect(subjects(upstream, "main")).toEqual(["theirs", "second", "first"]);
  });

  it("explains a push the remote declined itself, without offering to force it", async () => {
    const { upstream, path } = createClone("declined");
    git(upstream, "config", "receive.denyNonFastForwards", "true");
    commit(path, "second");
    git(path, "push", "-q");
    git(path, "commit", "-q", "--amend", "--allow-empty", "-m", "second, reworded");
    const error = await rejectionOf(push(await repos.open("declined"), { force: true }));
    expect(error).not.toBeInstanceOf(PushRejectedError);
    expect(error).toMatchObject({ message: expect.stringContaining("failed to push") });
  });

  it("says why it can't push without a branch, commits or a remote", async () => {
    const { path } = createClone("blocked");
    const repo = await repos.open("blocked");
    git(path, "checkout", "-q", "--detach");
    expect(await getPushTarget(repo)).toEqual({
      kind: "blocked",
      reason: "HEAD is detached: check out a branch to push it.",
    });
    await expect(push(repo, { force: false })).rejects.toThrow(PushBlockedError);

    git(path, "checkout", "-q", "--orphan", "empty");
    expect(await getPushTarget(repo)).toEqual({
      kind: "blocked",
      reason: "empty has no commits to push yet.",
    });

    const lonely = createRepo("blocked-lonely");
    commit(lonely, "first");
    expect(await getPushTarget(await repos.open("blocked-lonely"))).toEqual({
      kind: "blocked",
      reason: "There's no remote to push to.",
    });
  });
});

describe("rejection", () => {
  it("reads why git wouldn't push the ref from git push --porcelain", () => {
    const stdout = [
      "To /remote.git",
      "!\trefs/heads/other:refs/heads/other\t[rejected] (non-fast-forward)",
      "!\trefs/heads/hook:refs/heads/hook\t[remote rejected] (pre-receive hook declined)",
      "!\trefs/heads/main:refs/heads/trunk\t[rejected] (fetch first)",
      "Done",
    ].join("\n");
    expect(rejection(stdout, "refs/heads/main")).toBe("fetch first");
    // Not another ref's reason, nor the remote's own.
    expect(rejection(stdout, "refs/heads/elsewhere")).toBeUndefined();
    expect(rejection(stdout, "refs/heads/hook")).toBeUndefined();
  });
});
