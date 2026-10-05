import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { beforeAll, describe, expect, it, vi } from "vitest";

import { GitError } from "../../core/errors";
import type { Repo } from "../../core/repo";
import { createHistoryRepo, createRepo, git, page, rejection, repos } from "../../test/fixtures";
import { getCommit, getLog } from "./commands";

describe("a repository with history", () => {
  let repo: Repo;

  beforeAll(async () => {
    repo = await createHistoryRepo();
  });

  it("reads the log of branches and tags, but not other refs", async () => {
    const log = await getLog(repo, page);
    // Both sides of the merge have the same timestamp, so their order is up to git.
    expect(log.map((commit) => commit.subject).toSorted()).toEqual([
      "Merge side",
      "first",
      "main commit",
      "side commit",
    ]);
    expect(log[0]).toMatchObject({
      refs: [
        { kind: "local", name: "main", fullName: "refs/heads/main", current: true },
        { kind: "tag", name: "v1", fullName: "refs/tags/v1" },
      ],
      authorName: "Test User",
    });
    expect(log[0]!.parents).toHaveLength(2);
  });

  it("reads a single commit", async () => {
    const [head] = await getLog(repo, page);
    expect(await getCommit(repo, head!.sha.slice(0, 7))).toEqual(head);

    const tree = git(repo.path, "rev-parse", "HEAD^{tree}");
    const error = await rejection(getCommit(repo, tree));
    expect(error).toBeInstanceOf(GitError);
    expect(error).toMatchObject({ message: `${tree} is not a commit.` });
  });
});

describe("a repository without commits", () => {
  it("has an empty log", async () => {
    createRepo("empty");
    expect(await getLog(await repos.open("empty"), page)).toEqual([]);
  });
});

const chain = (path: string) => join(path, ".git/objects/info/commit-graphs/commit-graph-chain");

describe("the commit-graph", () => {
  it("is written after the log, once per run", async () => {
    const path = createRepo("graph");
    git(path, "commit", "-q", "--allow-empty", "-m", "first");
    const repo = await repos.open("graph");
    expect(existsSync(chain(path))).toBe(false);

    await getLog(repo, page);
    await vi.waitFor(() => expect(existsSync(chain(path))).toBe(true));
    const written = readFileSync(chain(path), "utf8");

    // A commit since then isn't added until the app runs again.
    git(path, "commit", "-q", "--allow-empty", "-m", "second");
    await getLog(repo, page);
    // Resolves once the write is done, without writing again.
    await repo.updateCommitGraph();
    expect(readFileSync(chain(path), "utf8")).toBe(written);
  });

  it("isn't needed to put a commit dated before its parent above it", async () => {
    const path = createRepo("graph-backdated");
    git(path, "commit", "-q", "--allow-empty", "-m", "parent");
    git(path, "checkout", "-qb", "side");
    // As a rebase with --committer-date-is-author-date leaves them. Unsorted, git lists it last.
    execFileSync("git", ["commit", "-q", "--allow-empty", "-m", "child"], {
      cwd: path,
      env: { ...process.env, GIT_COMMITTER_DATE: "2001-01-01T00:00:00Z" },
    });
    const repo = await repos.open("graph-backdated");
    const subjects = async () => (await getLog(repo, page)).map((commit) => commit.subject);

    expect(await subjects()).toEqual(["child", "parent"]);
    await repo.updateCommitGraph();
    expect(existsSync(chain(path))).toBe(true);
    expect(await subjects()).toEqual(["child", "parent"]);
  });

  it("doesn't have git sort a log that's already in order", async () => {
    const path = createRepo("graph-ordered");
    git(path, "commit", "-q", "--allow-empty", "-m", "first");
    const repo = await repos.open("graph-ordered");
    await repo.updateCommitGraph();
    expect(existsSync(chain(path))).toBe(true);

    const logs: string[][] = [];
    const counted: Repo = {
      ...repo,
      read: (args, options) => {
        if (args[0] === "log") logs.push(args);
        return repo.read(args, options);
      },
    };
    expect(await getLog(counted, page)).toHaveLength(1);
    expect(logs).toHaveLength(1);
    expect(logs[0]).not.toContain("--date-order");
  });

  it("has git sort a log that needed it straight away from then on", async () => {
    const path = createRepo("graph-remembered");
    git(path, "commit", "-q", "--allow-empty", "-m", "parent");
    execFileSync("git", ["commit", "-q", "--allow-empty", "-m", "child"], {
      cwd: path,
      env: { ...process.env, GIT_COMMITTER_DATE: "2001-01-01T00:00:00Z" },
    });
    git(path, "branch", "old", "HEAD~1");
    const repo = await repos.open("graph-remembered");
    const logs: boolean[] = [];
    const counted: Repo = {
      ...repo,
      read: (args, options) => {
        if (args[0] === "log") logs.push(args.includes("--date-order"));
        return repo.read(args, options);
      },
    };
    const subjects = async () => (await getLog(counted, page)).map((commit) => commit.subject);

    // Unsorted, then sorted.
    expect(await subjects()).toEqual(["child", "parent"]);
    expect(logs).toEqual([false, true]);
    expect(await subjects()).toEqual(["child", "parent"]);
    expect(logs).toEqual([false, true, true]);
  });

  it("skips to a later page in git's order, as each of them is", async () => {
    const path = createRepo("graph-paged");
    for (const subject of ["first", "second", "third"]) {
      git(path, "commit", "-q", "--allow-empty", "-m", subject);
    }
    const repo = await repos.open("graph-paged");
    const logs: boolean[] = [];
    const counted: Repo = {
      ...repo,
      read: (args, options) => {
        if (args[0] === "log") logs.push(args.includes("--date-order"));
        return repo.read(args, options);
      },
    };

    const later = await getLog(counted, { limit: 2, skip: 1 });
    expect(later.map((commit) => commit.subject)).toEqual(["second", "first"]);
    expect(logs).toEqual([true]);
  });

  it("doesn't fail the log when it can't be written", async () => {
    const path = createRepo("graph-locked");
    git(path, "commit", "-q", "--allow-empty", "-m", "first");
    // Another git is writing it.
    mkdirSync(join(path, ".git/objects/info/commit-graphs"), { recursive: true });
    writeFileSync(`${chain(path)}.lock`, "");
    const repo = await repos.open("graph-locked");

    expect(await getLog(repo, page)).toHaveLength(1);
    await repo.updateCommitGraph();
    expect(existsSync(chain(path))).toBe(false);
  });
});
