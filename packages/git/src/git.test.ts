// Runs every feature's commands against real repositories in a temp directory.
import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import type { RepositoryService } from "@gitto/repository/server";
import { afterAll, beforeAll, describe, expect, it } from "vitest";

import { GitReposImpl, type Repo } from "./core/repo";
import { createCommit } from "./features/commit/commands";
import { getCommitFiles, getWorkingTreeFiles } from "./features/diff/commands";
import { getLog } from "./features/history/commands";
import { listRefs } from "./features/refs/commands";
import { stage, unstage } from "./features/staging/commands";
import { getStatus } from "./features/status/commands";
import { watchChanges } from "./features/watch/commands";

let root: string;
const paths = new Map<string, string>();
const repos = new GitReposImpl({
  getRepository: async (id: string) => {
    const path = paths.get(id);
    return path ? { id, name: id, path } : undefined;
  },
} as RepositoryService);

function git(cwd: string, ...args: string[]) {
  return execFileSync("git", args, { cwd, encoding: "utf8" }).trim();
}

function createRepo(name: string): string {
  const path = join(root, name);
  git(root, "init", "-q", "-b", "main", name);
  git(path, "config", "user.name", "Test User");
  git(path, "config", "user.email", "test@example.com");
  git(path, "config", "commit.gpgsign", "false");
  paths.set(name, path);
  return path;
}

const page = { limit: 50, skip: 0 };

beforeAll(() => {
  root = mkdtempSync(join(tmpdir(), "gitto-git-"));
});

afterAll(() => {
  rmSync(root, { recursive: true, force: true });
});

describe("a repository with history", () => {
  let repo: Repo;
  const untrackedFile = {
    path: "new file.txt",
    status: "untracked",
    origPath: null,
    additions: null,
    deletions: null,
  };

  beforeAll(async () => {
    const path = createRepo("history");
    writeFileSync(join(path, "a file.txt"), "a\n");
    writeFileSync(join(path, "b.txt"), "b\n");
    writeFileSync(join(path, "bin.dat"), Buffer.from([0, 1, 2]));
    git(path, "add", "-A");
    git(path, "commit", "-qm", "first");
    git(path, "checkout", "-qb", "side");
    git(path, "mv", "b.txt", "c.txt");
    writeFileSync(join(path, "s.txt"), "s\n");
    git(path, "add", "-A");
    git(path, "commit", "-qm", "side commit");
    git(path, "checkout", "-q", "main");
    writeFileSync(join(path, "a file.txt"), "a\nmore\n");
    git(path, "commit", "-qam", "main commit");
    git(path, "merge", "-q", "side", "-m", "Merge side");
    git(path, "tag", "v1");
    // A commit only reachable from a tool's ref namespace, like T3 Code's checkpoints.
    const checkpoint = git(path, "commit-tree", "HEAD^{tree}", "-m", "checkpoint");
    git(path, "update-ref", "refs/tool/checkpoint", checkpoint);
    writeFileSync(join(path, "new file.txt"), "new\n");
    writeFileSync(join(path, "a file.txt"), "changed\n");
    repo = await repos.open("history");
  });

  it("reads the status", async () => {
    const status = await getStatus(repo);
    expect(status).toMatchObject({ branch: "main", upstream: null, ahead: 0, behind: 0 });
    expect(status.files).toEqual([
      { path: "a file.txt", origPath: null, staged: null, unstaged: "modified" },
      { path: "new file.txt", origPath: null, staged: null, unstaged: "untracked" },
    ]);
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
    expect(log[0]).toMatchObject({ refs: ["HEAD", "main", "tag: v1"], authorName: "Test User" });
    expect(log[0]!.parents).toHaveLength(2);
  });

  it("lists refs", async () => {
    const refs = await listRefs(repo);
    expect(refs.map((ref) => `${ref.kind}:${ref.name}${ref.current ? "*" : ""}`)).toEqual([
      "local:main*",
      "local:side",
      "tag:v1",
    ]);
  });

  it("diffs a merge against its first parent, and a root commit against nothing", async () => {
    const log = await getLog(repo, page);
    const merge = log.find((commit) => commit.subject === "Merge side");
    const first = log.find((commit) => commit.subject === "first");
    expect(await getCommitFiles(repo, merge!.sha)).toEqual([
      { path: "c.txt", status: "renamed", origPath: "b.txt", additions: 0, deletions: 0 },
      { path: "s.txt", status: "added", origPath: null, additions: 1, deletions: 0 },
    ]);
    expect(await getCommitFiles(repo, first!.sha)).toEqual([
      { path: "a file.txt", status: "added", origPath: null, additions: 1, deletions: 0 },
      { path: "b.txt", status: "added", origPath: null, additions: 1, deletions: 0 },
      { path: "bin.dat", status: "added", origPath: null, additions: null, deletions: null },
    ]);
  });

  it("diffs the working tree against the index", async () => {
    expect(await getWorkingTreeFiles(repo)).toEqual({
      staged: [],
      unstaged: [
        { path: "a file.txt", status: "modified", origPath: null, additions: 1, deletions: 2 },
        untrackedFile,
      ],
    });
  });

  it("moves staged changes between the working tree and the index", async () => {
    const change = {
      path: "a file.txt",
      status: "modified",
      origPath: null,
      additions: 1,
      deletions: 2,
    };

    await stage(repo, ["a file.txt"]);
    expect(await getWorkingTreeFiles(repo)).toEqual({
      staged: [change],
      unstaged: [untrackedFile],
    });

    await unstage(repo, ["a file.txt"]);
    expect(await getWorkingTreeFiles(repo)).toEqual({
      staged: [],
      unstaged: [change, untrackedFile],
    });
  });

  it("reports an unknown commit as an API error", async () => {
    await expect(
      getCommitFiles(repo, "0123456789abcdef0123456789abcdef01234567"),
    ).rejects.toMatchObject({
      code: "INTERNAL_SERVER_ERROR",
      message: expect.stringContaining("bad object"),
    });
  });
});

describe("a repository without commits", () => {
  let path: string;
  let repo: Repo;

  beforeAll(async () => {
    path = createRepo("empty");
    repo = await repos.open("empty");
  });

  it("has an empty status, log and ref list", async () => {
    expect(await getStatus(repo)).toMatchObject({ branch: "main", head: null, files: [] });
    expect(await getLog(repo, page)).toEqual([]);
    expect(await listRefs(repo)).toEqual([]);
    expect(await getWorkingTreeFiles(repo)).toEqual({ staged: [], unstaged: [] });
  });

  it("stages, unstages and commits", async () => {
    writeFileSync(join(path, "x y.txt"), "hi\n");

    await stage(repo, ["x y.txt"]);
    expect((await getStatus(repo)).files).toEqual([
      { path: "x y.txt", origPath: null, staged: "added", unstaged: null },
    ]);
    expect(await getWorkingTreeFiles(repo)).toEqual({
      staged: [{ path: "x y.txt", status: "added", origPath: null, additions: 1, deletions: 0 }],
      unstaged: [],
    });

    await unstage(repo, ["x y.txt"]);
    expect((await getStatus(repo)).files).toEqual([
      { path: "x y.txt", origPath: null, staged: null, unstaged: "untracked" },
    ]);

    await stage(repo, ["x y.txt"]);
    await createCommit(repo, "Initial commit\n\nWith body");
    expect(await getLog(repo, page)).toEqual([
      expect.objectContaining({
        subject: "Initial commit",
        body: "With body",
        refs: ["HEAD", "main"],
      }),
    ]);
  });

  it("explains why a commit failed", async () => {
    await expect(createCommit(repo, "nothing")).rejects.toMatchObject({
      message: expect.stringContaining("nothing to commit"),
    });
  });
});

describe("opening repositories", () => {
  it("rejects unknown repositories", async () => {
    await expect(repos.open("missing")).rejects.toMatchObject({ code: "NOT_FOUND" });
  });

  it("rejects repositories whose folder was deleted", async () => {
    const path = createRepo("deleted");
    rmSync(path, { recursive: true });
    await expect(repos.open("deleted")).rejects.toMatchObject({
      code: "NOT_FOUND",
      message: `${path} no longer exists.`,
    });
  });
});

describe("cancelling", () => {
  it("rejects with the abort instead of a git error", async () => {
    const repo = await repos.open("history");
    await expect(repo.read(["log"], { signal: AbortSignal.abort() })).rejects.toMatchObject({
      name: "AbortError",
    });
  });
});

describe("watching", () => {
  it("emits after a change and stops when aborted", async () => {
    const path = createRepo("watched");
    const repo = await repos.open("watched");
    const controller = new AbortController();
    const changes = watchChanges(repo, controller.signal);

    const next = changes.next();
    setTimeout(() => writeFileSync(join(path, "file.txt"), "x"), 50);
    expect(await next).toEqual({ value: null, done: false });

    controller.abort();
    expect(await changes.next()).toEqual({ value: undefined, done: true });
  });
});
