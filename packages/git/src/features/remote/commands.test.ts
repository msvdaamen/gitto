import { chmodSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";

import { GitError, NoUpstreamError, PullInterruptedError } from "../../core/errors";
import type { Repo } from "../../core/repo";
import { cloneRepo, createRepo, git, paths, rejection, repos, root } from "../../test/fixtures";
import { fetchAll, pull, remoteEnv } from "./commands";

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

afterEach(() => {
  vi.unstubAllEnvs();
});

describe("fetchAll", () => {
  let upstream: string;
  let path: string;
  let repo: Repo;

  // `upstream` plays the remote: a repository the clone fetches from as `origin`.
  const commitUpstream = (message: string) =>
    git(upstream, "commit", "--allow-empty", "-qm", message);
  const remoteBranches = () =>
    git(path, "for-each-ref", "--format=%(refname:lstrip=2) %(subject)", "refs/remotes").split(
      "\n",
    );

  beforeAll(async () => {
    upstream = join(root, "upstream");
    git(root, "init", "-q", "-b", "main", upstream);
    git(upstream, "config", "user.name", "Test User");
    git(upstream, "config", "user.email", "test@example.com");
    commitUpstream("first");
    git(upstream, "branch", "feature");
    path = join(root, "clone");
    git(root, "clone", "-q", upstream, path);
    paths.set("clone", path);
    repo = await repos.open("clone");
  });

  it("fetches new commits and drops branches deleted on the remote", async () => {
    commitUpstream("second");
    git(upstream, "branch", "-D", "feature");
    await fetchAll(repo);
    expect(remoteBranches()).toEqual(["origin/HEAD second", "origin/main second"]);
  });

  it("does nothing without remotes", async () => {
    git(root, "init", "-q", "lonely");
    paths.set("lonely", join(root, "lonely"));
    await expect(fetchAll(await repos.open("lonely"))).resolves.toBeUndefined();
  });

  it("explains why a fetch failed", async () => {
    git(path, "remote", "set-url", "origin", join(root, "gone"));
    await expect(fetchAll(repo)).rejects.toMatchObject({
      message: expect.stringContaining("does not appear to be a git repository"),
    });
  });
});

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
    expect(error).toBeInstanceOf(PullInterruptedError);
    expect(error).toMatchObject({
      message: "Pulling origin/main caused conflicts. Resolve them, then commit the merge.",
    });
    expect(git(path, "ls-files", "--unmerged")).not.toBe("");

    // Pulling again isn't the conflict's cause; git says why it can't.
    const again = await rejection(pull(await repos.open("conflicted")));
    expect(again).not.toBeInstanceOf(PullInterruptedError);
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

  it("rebases without opening an editor when the config says to rebase interactively", async () => {
    const { upstream, path } = createClone("interactive");
    git(path, "config", "pull.rebase", "interactive");
    commit(upstream, "theirs.txt", "theirs\n", "theirs");
    commit(path, "ours.txt", "ours\n", "ours");
    // An editor that fails, so an interactive rebase would too.
    vi.stubEnv("GIT_EDITOR", "false");
    vi.stubEnv("GIT_SEQUENCE_EDITOR", "false");
    await pull(await repos.open("interactive"));
    expect(subjects(path)).toEqual(["ours", "theirs", "first"]);
  });

  it("says when stashed local changes conflict with what it pulled", async () => {
    const { upstream, path } = createClone("autostash");
    git(path, "config", "pull.rebase", "true");
    git(path, "config", "rebase.autoStash", "true");
    commit(upstream, "a.txt", "theirs\n", "theirs");
    writeFileSync(join(path, "a.txt"), "uncommitted\n");
    await expect(pull(await repos.open("autostash"))).rejects.toMatchObject({
      name: "PullInterruptedError",
      message:
        "Pulled origin/main, but your local changes conflict with it. Resolve the conflicts; your changes are also kept in the stash.",
    });
    expect(subjects(path)).toEqual(["theirs", "first"]);
  });

  it("leaves git's hints out of why it failed", async () => {
    const { upstream, path } = createClone("ff-only");
    git(path, "config", "pull.ff", "only");
    commit(upstream, "theirs.txt", "theirs\n", "theirs");
    commit(path, "ours.txt", "ours\n", "ours");
    await expect(pull(await repos.open("ff-only"))).rejects.toMatchObject({
      message: "fatal: Not possible to fast-forward, aborting.",
    });
  });

  it("doesn't let ssh ask questions, unless the user set their own ssh command", async () => {
    // An `ssh` that records how it was run, and fails.
    const bin = join(root, "bin");
    const log = join(root, "ssh.log");
    mkdirSync(bin, { recursive: true });
    writeFileSync(join(bin, "ssh"), `#!/bin/sh\necho "$@" >> "${log}"\nexit 1\n`);
    chmodSync(join(bin, "ssh"), 0o755);
    vi.stubEnv("PATH", `${bin}:${process.env.PATH}`);
    vi.stubEnv("GIT_SSH_COMMAND", undefined);
    vi.stubEnv("GIT_SSH", undefined);
    vi.stubEnv("SSH_ASKPASS", undefined);

    const { path } = createClone("ssh");
    git(path, "remote", "set-url", "origin", "git@example.invalid:repo.git");
    const repo = await repos.open("ssh");
    await expect(pull(repo)).rejects.toBeInstanceOf(GitError);
    expect(readFileSync(log, "utf8")).toContain("-o BatchMode=yes");

    writeFileSync(log, "");
    git(path, "config", "core.sshCommand", "ssh -o User=me");
    await expect(pull(repo)).rejects.toBeInstanceOf(GitError);
    const run = readFileSync(log, "utf8");
    expect(run).toContain("-o User=me");
    expect(run).not.toContain("BatchMode");
  });

  it("says how to finish a rebase that stopped without conflicts", async () => {
    const { upstream, path } = createClone("rebase-stopped");
    git(path, "config", "pull.rebase", "true");
    commit(upstream, "theirs.txt", "theirs\n", "theirs");
    // Replaying the first commit would overwrite the untracked file the second one leaves.
    commit(path, "gen.txt", "gen\n", "add gen");
    git(path, "rm", "-q", "--cached", "gen.txt");
    git(path, "commit", "-qm", "untrack gen");
    writeFileSync(join(path, "gen.txt"), "untracked\n");
    const error = await rejection(pull(await repos.open("rebase-stopped")));
    expect(error).toBeInstanceOf(PullInterruptedError);
    expect((error as Error).message).toMatch(
      /^Rebasing onto origin\/main stopped partway:\n.*untracked working tree files would be overwritten/,
    );
    expect((error as Error).message).toMatch(/Fix that, then continue the rebase, or abort it\.$/);
    expect((error as Error).message).not.toContain("hint:");
  });

  it("pulls a branch that has a tag of the same name", async () => {
    const { upstream, path } = createClone("tagged");
    git(path, "tag", "main");
    commit(upstream, "a.txt", "a\nb\n", "second");
    await pull(await repos.open("tagged"));
    expect(subjects(path)).toEqual(["second", "first"]);
  });

  it("pulls from a local upstream", async () => {
    const path = createRepo("local-upstream");
    commit(path, "a.txt", "a\n", "first");
    git(path, "checkout", "-qb", "feature", "--track", "main");
    git(path, "checkout", "-q", "main");
    commit(path, "a.txt", "a\nb\n", "second");
    git(path, "checkout", "-q", "feature");
    await pull(await repos.open("local-upstream"));
    expect(subjects(path)).toEqual(["second", "first"]);
  });

  it("doesn't merge into another branch checked out while it fetched", async () => {
    const { upstream, path } = createClone("switched");
    commit(upstream, "a.txt", "a\nb\n", "second");
    git(path, "branch", "-q", "--track", "other", "origin/main");
    const real = await repos.open("switched");
    const repo: Repo = {
      ...real,
      fetch: async (args, options) => {
        const output = await real.fetch(args, options);
        git(path, "checkout", "-q", "other");
        return output;
      },
    };
    await expect(pull(repo)).rejects.toMatchObject({
      message: "Switched from main while pulling it. Pull again.",
    });
    expect(subjects(path)).toEqual(["first"]);
  });

  it("says to continue a rebase that stopped in a linked worktree", async () => {
    const { upstream, path } = createClone("worktree-main");
    commit(upstream, "a.txt", "theirs\n", "theirs");
    const worktree = join(root, "worktree");
    git(path, "worktree", "add", "-q", "-b", "wt", worktree, "origin/main~0");
    git(worktree, "branch", "-q", "--set-upstream-to", "origin/main");
    git(worktree, "reset", "-q", "--hard", "main");
    git(worktree, "config", "pull.rebase", "true");
    commit(worktree, "a.txt", "ours\n", "ours");
    paths.set("worktree", worktree);
    await expect(pull(await repos.open("worktree"))).rejects.toMatchObject({
      message: "Pulling origin/main caused conflicts. Resolve them, then continue the rebase.",
    });
  });

  it("pulls an upstream that has no remote-tracking branch", async () => {
    const upstream = createRepo("untracked-upstream");
    commit(upstream, "a.txt", "a\n", "first");
    git(upstream, "branch", "dev");
    const path = cloneRepo("untracked", upstream);
    // Only `main` is fetched into a remote-tracking branch.
    git(path, "config", "remote.origin.fetch", "+refs/heads/main:refs/remotes/origin/main");
    git(path, "checkout", "-qb", "dev");
    git(path, "config", "branch.dev.remote", "origin");
    git(path, "config", "branch.dev.merge", "refs/heads/dev");
    git(upstream, "checkout", "-q", "dev");
    commit(upstream, "theirs.txt", "theirs\n", "theirs");
    commit(path, "ours.txt", "ours\n", "ours");
    await pull(await repos.open("untracked"));
    expect(git(path, "log", "-1", "--format=%s")).toBe("Merge branch 'dev' of origin");
    expect(subjects(path)).toEqual(expect.arrayContaining(["theirs", "ours"]));
  });

  it("pulls the first commits into a branch without any, even when set to rebase", async () => {
    const upstream = createRepo("empty-upstream");
    const path = cloneRepo("empty", upstream);
    git(path, "config", "pull.rebase", "true");
    git(path, "config", "branch.main.remote", "origin");
    git(path, "config", "branch.main.merge", "refs/heads/main");
    commit(upstream, "a.txt", "a\n", "first");
    await pull(await repos.open("empty"));
    expect(subjects(path)).toEqual(["first"]);
  });

  it("doesn't merge another upstream than the one it fetched", async () => {
    const { upstream, path } = createClone("upstream-changed");
    commit(upstream, "a.txt", "a\nb\n", "second");
    const real = await repos.open("upstream-changed");
    const repo: Repo = {
      ...real,
      fetch: async (args, options) => {
        const output = await real.fetch(args, options);
        git(path, "config", "branch.main.merge", "refs/heads/release");
        return output;
      },
    };
    await expect(pull(repo)).rejects.toMatchObject({
      message: "main's upstream changed while pulling it. Pull again.",
    });
    expect(subjects(path)).toEqual(["first"]);
  });

  it("explains why local changes stop it, without git's hints", async () => {
    const { upstream, path } = createClone("dirty");
    commit(upstream, "a.txt", "theirs\n", "theirs");
    writeFileSync(join(path, "a.txt"), "uncommitted\n");
    const error = await rejection(pull(await repos.open("dirty")));
    expect(error).toMatchObject({
      message: expect.stringContaining("Your local changes to the following files would be"),
    });
    expect(error).not.toBeInstanceOf(PullInterruptedError);
    expect(readFileSync(join(path, "a.txt"), "utf8")).toBe("uncommitted\n");
  });

  it("needs a branch that tracks a remote one", async () => {
    const path = createRepo("local");
    commit(path, "a.txt", "a\n", "first");
    const repo = await repos.open("local");
    await expect(pull(repo)).rejects.toEqual(
      new NoUpstreamError("main doesn't track a remote branch."),
    );

    git(path, "checkout", "-q", "--detach");
    await expect(pull(repo)).rejects.toEqual(
      new NoUpstreamError("Check out a branch to pull into it."),
    );
  });
});

describe("the environment for talking to remotes", () => {
  const none = new Map<string, string>();

  it("keeps ssh from asking, and gives up on a stalled transfer", () => {
    expect(remoteEnv(none, {})).toEqual({
      GIT_SSH_COMMAND: "ssh -o BatchMode=yes",
      GIT_HTTP_LOW_SPEED_LIMIT: "1000",
      GIT_HTTP_LOW_SPEED_TIME: "60",
    });
  });

  it("asks with the user's askpass program instead, if there is one", () => {
    expect(remoteEnv(none, { SSH_ASKPASS: "/usr/bin/ksshaskpass" })).toMatchObject({
      SSH_ASKPASS_REQUIRE: "force",
    });
    expect(remoteEnv(none, { SSH_ASKPASS: "/usr/bin/ksshaskpass" })).not.toHaveProperty(
      "GIT_SSH_COMMAND",
    );
    expect(
      remoteEnv(none, { SSH_ASKPASS: "askpass", SSH_ASKPASS_REQUIRE: "prefer" }),
    ).not.toHaveProperty("SSH_ASKPASS_REQUIRE");
  });

  it("leaves the user's own settings be", () => {
    expect(remoteEnv(none, { GIT_SSH: "plink", GIT_HTTP_LOW_SPEED_TIME: "600" })).toEqual({});
    expect(
      remoteEnv(none, { GIT_SSH_COMMAND: "ssh -i key", GIT_HTTP_LOW_SPEED_LIMIT: "1" }),
    ).toEqual({});
    for (const key of ["http.lowspeedtime", "http.https://example.com/.lowspeedlimit"]) {
      expect(
        remoteEnv(
          new Map([
            ["core.sshcommand", "ssh"],
            [key, "1"],
          ]),
          {},
        ),
      ).toEqual({});
    }
  });
});
