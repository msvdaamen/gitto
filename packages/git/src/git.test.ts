// Runs every feature's commands against real repositories in a temp directory.
import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import type { RepositoryService } from "@gitto/repository/server";
import { call, ORPCError } from "@orpc/server";
import { afterAll, beforeAll, describe, expect, it } from "vitest";

import {
  FolderNotFoundError,
  GitError,
  IndexLockedError,
  NotARepositoryError,
  RepositoryNotFoundError,
} from "./core/errors";
import { GitReposImpl, type Repo } from "./core/repo";
import { createCommit } from "./features/commit/commands";
import { getCommitFiles, getWorkingTreeFiles } from "./features/diff/commands";
import { getCommit, getLog } from "./features/history/commands";
import { listRefs } from "./features/refs/commands";
import { stage, unstage } from "./features/staging/commands";
import { getStatus } from "./features/status/commands";
import { watchGitDir } from "./features/watch/commands";
import { gitRouter } from "./router";

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

/** What `promise` rejects with; fails if it resolves. */
function rejection(promise: Promise<unknown>): Promise<unknown> {
  return promise.then(
    () => expect.fail("expected a rejection"),
    (reason: unknown) => reason,
  );
}

async function apiError(promise: Promise<unknown>): Promise<unknown> {
  const error = await rejection(promise);
  expect(error).toBeInstanceOf(ORPCError);
  return error;
}

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
    expect(status).toMatchObject({
      head: { kind: "branch", name: "main" },
      upstream: null,
      ahead: 0,
      behind: 0,
    });
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

  it("explains why a command failed", async () => {
    const error = await rejection(getCommitFiles(repo, "0123456789abcdef0123456789abcdef01234567"));
    expect(error).toBeInstanceOf(GitError);
    expect(error).toMatchObject({ message: expect.stringContaining("bad object") });
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
    expect(await getStatus(repo)).toMatchObject({
      head: { kind: "unborn", name: "main" },
      files: [],
    });
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
        refs: [{ kind: "local", name: "main", current: true }],
      }),
    ]);
  });

  it("explains why a commit failed", async () => {
    await expect(createCommit(repo, "nothing")).rejects.toMatchObject({
      message: expect.stringContaining("nothing to commit"),
    });
  });
});

describe("errors", () => {
  it("rejects unknown repositories", async () => {
    await expect(repos.open("missing")).rejects.toBeInstanceOf(RepositoryNotFoundError);
  });

  it("rejects repositories whose folder was deleted", async () => {
    const path = createRepo("deleted");
    rmSync(path, { recursive: true });
    const error = await rejection(repos.open("deleted"));
    expect(error).toBeInstanceOf(FolderNotFoundError);
    expect(error).toMatchObject({ message: `${path} no longer exists.` });
  });

  it("recognises a folder that's no longer a repository", async () => {
    const path = createRepo("unrepo");
    const repo = await repos.open("unrepo");
    rmSync(join(path, ".git"), { recursive: true });
    const error = await rejection(getStatus(repo));
    expect(error).toBeInstanceOf(NotARepositoryError);
    expect(error).toMatchObject({ message: `${path} is no longer a git repository.` });
  });

  it("recognises another git process holding the index", async () => {
    const path = createRepo("locked");
    const repo = await repos.open("locked");
    writeFileSync(join(path, "file.txt"), "x");
    writeFileSync(join(path, ".git", "index.lock"), "");
    await expect(stage(repo, ["file.txt"])).rejects.toBeInstanceOf(IndexLockedError);
  });
});

describe("the router", () => {
  // Procedures only take UUIDv7 repository ids.
  const ids = {
    history: "01920000-0000-7000-8000-000000000001",
    locked: "01920000-0000-7000-8000-000000000002",
    missing: "01920000-0000-7000-8000-000000000003",
  };
  const context = { gitRepos: repos };

  beforeAll(() => {
    paths.set(ids.history, paths.get("history")!);
    paths.set(ids.locked, paths.get("locked")!);
  });

  it("reports git errors as API errors the renderer can show", async () => {
    expect(
      await apiError(call(gitRouter.status.get, { repositoryId: ids.missing }, { context })),
    ).toMatchObject({ code: "NOT_FOUND", message: "Repository not found." });

    expect(
      await apiError(
        call(
          gitRouter.staging.stage,
          { repositoryId: ids.locked, paths: ["file.txt"] },
          { context },
        ),
      ),
    ).toMatchObject({
      code: "CONFLICT",
      message: "Another git process is running in this repository.",
    });

    expect(
      await apiError(
        call(
          gitRouter.diff.commitFiles,
          { repositoryId: ids.history, sha: "0123456789abcdef0123456789abcdef01234567" },
          { context },
        ),
      ),
    ).toMatchObject({
      code: "INTERNAL_SERVER_ERROR",
      message: expect.stringContaining("bad object"),
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

/** Whether `next` stays pending for longer than the watcher's debounce: resolves to "quiet" if so. */
function quiet(next: Promise<unknown>): Promise<unknown> {
  return Promise.race([next, new Promise((resolve) => setTimeout(() => resolve("quiet"), 700))]);
}

/** Runs `change` once the watcher has had time to start; it only sees what happens after that. */
function soon(change: () => void) {
  setTimeout(change, 200);
}

describe("watching the git directory", () => {
  it(
    "reports staging, commits and checkouts, but not the working tree",
    { timeout: 10_000 },
    async () => {
      const path = createRepo("watched-refs");
      const repo = await repos.open("watched-refs");
      const controller = new AbortController();
      const changes = watchGitDir(repo, controller.signal);

      let next = changes.next();
      soon(() => {
        writeFileSync(join(path, "file.txt"), "x");
        git(path, "add", "file.txt");
      });
      expect(await next).toEqual({ value: ["index"], done: false });

      next = changes.next();
      git(path, "commit", "-q", "-m", "First");
      expect((await next).value).toContain("refs");

      next = changes.next();
      git(path, "checkout", "-q", "-b", "feature");
      expect((await next).value).toContain("refs");

      next = changes.next();
      writeFileSync(join(path, "file.txt"), "y");
      expect(await quiet(next)).toBe("quiet");

      controller.abort();
      expect(await changes.next()).toEqual({ value: undefined, done: true });
    },
  );

  it("reports refs packed into packed-refs", { timeout: 10_000 }, async () => {
    const path = createRepo("watched-packed");
    git(path, "commit", "-q", "--allow-empty", "-m", "First");
    git(path, "tag", "v1");
    const repo = await repos.open("watched-packed");
    const controller = new AbortController();
    const changes = watchGitDir(repo, controller.signal);

    const next = changes.next();
    soon(() => git(path, "pack-refs", "--all"));
    expect(await next).toEqual({ value: ["refs"], done: false });

    controller.abort();
  });

  it("follows a linked worktree's own HEAD and index", { timeout: 10_000 }, async () => {
    const main = createRepo("watched-main");
    git(main, "commit", "-q", "--allow-empty", "-m", "First");
    const linked = join(root, "watched-linked");
    git(main, "worktree", "add", "-q", linked);
    paths.set("watched-linked", linked);
    const repo = await repos.open("watched-linked");
    const controller = new AbortController();
    const changes = watchGitDir(repo, controller.signal);

    let next = changes.next();
    soon(() => {
      writeFileSync(join(linked, "file.txt"), "x");
      git(linked, "add", "file.txt");
    });
    expect(await next).toEqual({ value: ["index"], done: false });

    // The main worktree's index isn't this one's.
    next = changes.next();
    writeFileSync(join(main, "other.txt"), "x");
    git(main, "add", "other.txt");
    expect(await quiet(next)).toBe("quiet");

    // Branches are shared.
    git(main, "branch", "shared");
    expect((await next).value).toContain("refs");

    controller.abort();
  });
});
