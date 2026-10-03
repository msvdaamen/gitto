import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { beforeAll, describe, expect, it, vi } from "vitest";

import { GitError } from "../../core/errors";
import type { Repo } from "../../core/repo";
import { createHistoryRepo, createRepo, git, page, rejection, repos } from "../../test/fixtures";
import { getCommit, getLog, getVersionedLog } from "./commands";

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
        { kind: "local", name: "main", current: true },
        { kind: "tag", name: "v1" },
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
    expect(await getVersionedLog(await repos.open("empty"), page)).toMatchObject({ commits: [] });
  });
});

describe("the log's version", () => {
  it("skips a log that would be the same, and reads one that wouldn't", async () => {
    const path = createRepo("versioned");
    git(path, "commit", "-q", "--allow-empty", "-m", "first");
    const repo = await repos.open("versioned");
    /** The version of the log now, checking it's read as `getLog` reads it. */
    const version = async (pageOfLog = page) => {
      const log = await getVersionedLog(repo, pageOfLog);
      if ("unchanged" in log) return expect.fail("expected a log");
      expect(log.commits).toEqual(await getLog(repo, pageOfLog));
      return log.version;
    };

    let last = await version();
    expect(await getVersionedLog(repo, page, last)).toEqual({ unchanged: true });
    // Nothing the log shows.
    writeFileSync(join(path, "file.txt"), "x");
    git(path, "add", "file.txt");
    git(path, "config", "core.something", "else");
    expect(await getVersionedLog(repo, page, last)).toEqual({ unchanged: true });

    const changes: [string, () => void][] = [
      ["a commit", () => git(path, "commit", "-q", "-m", "second")],
      // On the same commit, but HEAD's decoration moves.
      ["switching branches", () => git(path, "switch", "-q", "-c", "other")],
      ["detaching HEAD", () => git(path, "switch", "-q", "--detach")],
      ["a tag", () => git(path, "tag", "v1")],
      ["a branch deleted", () => git(path, "branch", "-q", "-D", "other")],
    ];
    for (const [change, run] of changes) {
      run();
      // oxlint-disable-next-line no-await-in-loop -- each change follows the one before.
      const next = await version();
      expect(next, change).not.toBe(last);
      last = next;
    }
    expect(await version({ limit: 1, skip: 0 })).not.toBe(last);
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
