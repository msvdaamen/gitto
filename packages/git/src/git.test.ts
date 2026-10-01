// Runs every feature's commands against real repositories in a temp directory.
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, rmSync, utimesSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import type { RepositoryService } from "@gitto/repository/server";
import { call, ORPCError } from "@orpc/server";
import { afterAll, beforeAll, describe, expect, it, vi } from "vitest";

import {
  FolderNotFoundError,
  GitError,
  IndexLockedError,
  NotARepositoryError,
  RepositoryNotFoundError,
} from "./core/errors";
import { GitReposImpl, type Repo } from "./core/repo";
import { createCommit } from "./features/commit/commands";
import { getCommitFiles } from "./features/diff/commands";
import { parseDiff } from "./features/diff/parse";
import { getCommit, getLog } from "./features/history/commands";
import { listRefs } from "./features/refs/commands";
import { stage, stageAll, unstage, unstageAll } from "./features/staging/commands";
import { getStatus } from "./features/status/commands";
import { watchGitDir, watchWorkingTree } from "./features/watch/commands";
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
    expect((await getStatus(repo)).changes).toEqual({
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
    expect((await getStatus(repo)).changes).toEqual({
      staged: [change],
      unstaged: [untrackedFile],
    });

    await unstage(repo, ["a file.txt"]);
    expect((await getStatus(repo)).changes).toEqual({
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
    expect((await getStatus(repo)).changes).toEqual({ staged: [], unstaged: [] });
  });

  it("stages, unstages and commits", async () => {
    writeFileSync(join(path, "x y.txt"), "hi\n");

    await stage(repo, ["x y.txt"]);
    expect((await getStatus(repo)).files).toEqual([
      { path: "x y.txt", origPath: null, staged: "added", unstaged: null },
    ]);
    expect((await getStatus(repo)).changes).toEqual({
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

/** What the changed files were before they came from the status: full diffs and ls-files. */
async function fullDiffs(repo: Repo) {
  const [staged, unstaged, untracked] = await Promise.all([
    repo.read(["diff", "--cached", "--raw", "--numstat", "-z", "-M"]),
    repo.read(["diff", "--raw", "--numstat", "-z"]),
    repo.read(["ls-files", "--others", "--exclude-standard", "-z"]),
  ]);
  return {
    staged: parseDiff(staged),
    unstaged: [
      ...parseDiff(unstaged),
      ...untracked
        .split("\0")
        .filter(Boolean)
        .map((path) => ({
          path,
          status: "untracked",
          origPath: null,
          additions: null,
          deletions: null,
        })),
    ],
  };
}

describe("the changed files", () => {
  it("are the same as full diffs", async () => {
    const path = createRepo("changes");
    // `:colon.txt` would be pathspec magic, if git didn't take paths literally (GIT_LITERAL_PATHSPECS).
    for (const name of ["keep.txt", "edit.txt", "both.txt", "gone.txt", "move.txt", ":colon.txt"]) {
      writeFileSync(join(path, name), `${name}\n`);
    }
    writeFileSync(join(path, "bin.dat"), Buffer.from([0, 1, 2]));
    git(path, "add", ".");
    git(path, "commit", "-q", "-m", "first");

    writeFileSync(join(path, "edit.txt"), "changed\n");
    writeFileSync(join(path, ":colon.txt"), "changed\n");
    writeFileSync(join(path, "both.txt"), "staged\n");
    git(path, "add", "both.txt");
    writeFileSync(join(path, "both.txt"), "staged, then changed again\n");
    rmSync(join(path, "gone.txt"));
    git(path, "mv", "move.txt", "moved.txt");
    writeFileSync(join(path, "bin.dat"), Buffer.from([3, 4, 5]));
    writeFileSync(join(path, "new.txt"), "new\n");
    const repo = await repos.open("changes");

    const { changes } = await getStatus(repo);
    expect(changes).toEqual(await fullDiffs(repo));
    expect(changes.unstaged.map((file) => file.path)).toEqual(
      expect.arrayContaining([
        "edit.txt",
        ":colon.txt",
        "both.txt",
        "gone.txt",
        "bin.dat",
        "new.txt",
      ]),
    );
  });

  it("are the same as full diffs with a merge conflict", async () => {
    const path = createRepo("conflict");
    writeFileSync(join(path, "file.txt"), "base\n");
    git(path, "add", ".");
    git(path, "commit", "-q", "-m", "base");
    git(path, "checkout", "-q", "-b", "side");
    writeFileSync(join(path, "file.txt"), "side\n");
    git(path, "commit", "-q", "-am", "side");
    git(path, "checkout", "-q", "main");
    writeFileSync(join(path, "file.txt"), "main\n");
    git(path, "commit", "-q", "-am", "main");
    expect(() => git(path, "merge", "-q", "side")).toThrow();
    const repo = await repos.open("conflict");

    const { changes } = await getStatus(repo);
    expect(changes).toEqual(await fullDiffs(repo));
    expect(changes.unstaged).toEqual([expect.objectContaining({ path: "file.txt" })]);
  });

  it("are the same as full diffs when there are too many paths to list", async () => {
    const path = createRepo("many");
    const names = Array.from({ length: 400 }, (_, i) => `${"long-file-name-".repeat(3)}${i}.txt`);
    for (const name of names) writeFileSync(join(path, name), "a\n");
    git(path, "add", ".");
    git(path, "commit", "-q", "-m", "many");
    for (const name of names) writeFileSync(join(path, name), "b\n");
    const repo = await repos.open("many");

    const { changes } = await getStatus(repo);
    expect(changes.unstaged).toHaveLength(400);
    expect(changes).toEqual(await fullDiffs(repo));
  });
});

describe("a big working tree", () => {
  it("stages and unstages more paths than fit on a Windows command line", async () => {
    const path = createRepo("big-stage");
    const names = Array.from({ length: 1000 }, (_, i) => `${"long-file-name-".repeat(3)}${i}.txt`);
    for (const name of names) writeFileSync(join(path, name), "a\n");
    const repo = await repos.open("big-stage");

    await stage(repo, names);
    expect((await getStatus(repo)).changes.staged).toHaveLength(1000);
    await unstage(repo, names);
    expect((await getStatus(repo)).changes).toMatchObject({ staged: [] });
  });

  it("stages and unstages everything at once", async () => {
    const path = createRepo("all");
    for (const name of ["kept.txt", "deleted.txt", "moved.txt"]) {
      writeFileSync(join(path, name), `${name}\n`);
    }
    git(path, "add", ".");
    git(path, "commit", "-q", "-m", "First");
    writeFileSync(join(path, "kept.txt"), "changed\n");
    rmSync(join(path, "deleted.txt"));
    git(path, "mv", "moved.txt", "renamed.txt");
    writeFileSync(join(path, "new file.txt"), "new\n");
    const repo = await repos.open("all");

    await stageAll(repo);
    const staged = await getStatus(repo);
    expect(staged.files.map((file) => [file.path, file.staged, file.unstaged])).toEqual([
      ["deleted.txt", "deleted", null],
      ["kept.txt", "modified", null],
      ["new file.txt", "added", null],
      ["renamed.txt", "renamed", null],
    ]);

    await unstageAll(repo);
    const unstaged = await getStatus(repo);
    expect(unstaged.changes.staged).toEqual([]);
    expect(unstaged.files.map((file) => [file.path, file.unstaged])).toEqual([
      ["deleted.txt", "deleted"],
      ["kept.txt", "modified"],
      ["moved.txt", "deleted"],
      ["new file.txt", "untracked"],
      ["renamed.txt", "untracked"],
    ]);
  });

  it("unstages everything but conflicts, which stay conflicted", async () => {
    const path = createRepo("all-conflict");
    writeFileSync(join(path, "both.txt"), "base\n");
    writeFileSync(join(path, "other.txt"), "other\n");
    git(path, "add", ".");
    git(path, "commit", "-q", "-m", "base");
    git(path, "checkout", "-q", "-b", "side");
    writeFileSync(join(path, "both.txt"), "side\n");
    git(path, "commit", "-q", "-am", "side");
    git(path, "checkout", "-q", "main");
    writeFileSync(join(path, "both.txt"), "main\n");
    git(path, "commit", "-q", "-am", "main");
    expect(() => git(path, "merge", "-q", "side")).toThrow();
    writeFileSync(join(path, "other.txt"), "changed\n");
    git(path, "add", "other.txt");
    const repo = await repos.open("all-conflict");

    await unstageAll(repo);
    expect((await getStatus(repo)).files).toEqual([
      { path: "other.txt", origPath: null, staged: null, unstaged: "modified" },
      { path: "both.txt", origPath: null, staged: "conflicted", unstaged: "conflicted" },
    ]);
    expect(existsSync(join(path, ".git", "MERGE_HEAD"))).toBe(true);
  });

  it("stages and unstages everything before the first commit", async () => {
    const path = createRepo("all-empty");
    const repo = await repos.open("all-empty");
    await unstageAll(repo);

    writeFileSync(join(path, "a.txt"), "a\n");
    await stageAll(repo);
    expect((await getStatus(repo)).changes.staged).toHaveLength(1);
    await unstageAll(repo);
    expect((await getStatus(repo)).files).toEqual([
      { path: "a.txt", origPath: null, staged: null, unstaged: "untracked" },
    ]);
  });

  it("refreshes the index, so files touched but not changed aren't read again", async () => {
    const path = createRepo("touched");
    writeFileSync(join(path, "file.txt"), "a\n");
    git(path, "add", "file.txt");
    git(path, "commit", "-q", "-m", "First");
    const later = new Date(Date.now() + 60_000);
    utimesSync(join(path, "file.txt"), later, later);
    const repo = await repos.open("touched");

    // `diff-files` compares the index's timestamps without refreshing it.
    expect(git(path, "diff-files", "--name-only")).toBe("file.txt");
    await repo.refreshIndex();
    expect(git(path, "diff-files", "--name-only")).toBe("");
  });

  it("doesn't fail to refresh the index while another git process holds it", async () => {
    const path = createRepo("refresh-locked");
    const repo = await repos.open("refresh-locked");
    writeFileSync(join(path, ".git", "index.lock"), "");
    await expect(repo.refreshIndex()).resolves.toBeUndefined();
  });

  it("gets a commit-graph once its log is read", async () => {
    const path = createRepo("no-graph");
    git(path, "commit", "-q", "--allow-empty", "-m", "First");
    const graph = join(path, ".git", "objects", "info", "commit-graph");
    expect(existsSync(graph)).toBe(false);

    await getLog(await repos.open("no-graph"), page);
    await vi.waitFor(() => expect(existsSync(graph)).toBe(true));
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

describe("watching the working tree", () => {
  it("emits after a change and stops when aborted", async () => {
    const path = createRepo("watched");
    const repo = await repos.open("watched");
    const controller = new AbortController();
    const changes = watchWorkingTree(repo, controller.signal);

    const next = changes.next();
    soon(() => writeFileSync(join(path, "file.txt"), "x"));
    expect(await next).toEqual({ value: null, done: false });

    controller.abort();
    expect(await changes.next()).toEqual({ value: undefined, done: true });
  });

  it("leaves the git directory out", { timeout: 10_000 }, async () => {
    const path = createRepo("watched-git-dir");
    const repo = await repos.open("watched-git-dir");
    const controller = new AbortController();
    const changes = watchWorkingTree(repo, controller.signal);

    let next = changes.next();
    soon(() => writeFileSync(join(path, "file.txt"), "x"));
    expect(await next).toEqual({ value: null, done: false });

    next = changes.next();
    git(path, "add", "file.txt");
    expect(await quiet(next)).toBe("quiet");

    controller.abort();
  });

  it("skips ignored directories and follows new ones", { timeout: 10_000 }, async () => {
    const path = createRepo("watched-tree");
    writeFileSync(join(path, ".gitignore"), "build/\n");
    mkdirSync(join(path, "build"));
    const repo = await repos.open("watched-tree");
    const controller = new AbortController();
    const changes = watchWorkingTree(repo, controller.signal);

    let next = changes.next();
    soon(() => writeFileSync(join(path, "file.txt"), "x"));
    expect(await next).toEqual({ value: null, done: false });

    // Ignored.
    next = changes.next();
    writeFileSync(join(path, "build", "out.txt"), "x");
    expect(await quiet(next)).toBe("quiet");

    // Created (and reported) afterwards; changes in it are reported too.
    mkdirSync(join(path, "src"));
    expect(await next).toEqual({ value: null, done: false });
    next = changes.next();
    writeFileSync(join(path, "src", "a.txt"), "x");
    expect(await next).toEqual({ value: null, done: false });

    controller.abort();
  });

  it("follows changes to the ignore rules", { timeout: 10_000 }, async () => {
    const path = createRepo("watched-rules");
    writeFileSync(join(path, ".gitignore"), "build/\n");
    mkdirSync(join(path, "build"));
    mkdirSync(join(path, "src"));
    const repo = await repos.open("watched-rules");
    const controller = new AbortController();
    const changes = watchWorkingTree(repo, controller.signal);

    let next = changes.next();
    soon(() => writeFileSync(join(path, "file.txt"), "x"));
    expect(await next).toEqual({ value: null, done: false });

    // `build` is no longer ignored, `src` now is.
    next = changes.next();
    writeFileSync(join(path, ".gitignore"), "src/\n");
    expect(await next).toEqual({ value: null, done: false });

    next = changes.next();
    writeFileSync(join(path, "build", "out.txt"), "x");
    expect(await next).toEqual({ value: null, done: false });

    next = changes.next();
    writeFileSync(join(path, "src", "a.txt"), "x");
    expect(await quiet(next)).toBe("quiet");

    // Rules in .git/info/exclude count too.
    writeFileSync(join(path, ".git", "info", "exclude"), "build/\n");
    expect(await next).toEqual({ value: null, done: false });
    next = changes.next();
    writeFileSync(join(path, "build", "out.txt"), "y");
    expect(await quiet(next)).toBe("quiet");

    controller.abort();
  });
});

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

  it(
    "keeps going while the working tree is watched and unwatched",
    { timeout: 10_000 },
    async () => {
      const path = createRepo("watched-both");
      const repo = await repos.open("watched-both");
      const controller = new AbortController();
      const changes = watchGitDir(repo, controller.signal);
      let next = changes.next();

      // Like the UI does when the window gets and loses focus.
      const tree = new AbortController();
      const treeChanges = watchWorkingTree(repo, tree.signal);
      const treeNext = treeChanges.next();
      soon(() => writeFileSync(join(path, "file.txt"), "x"));
      expect(await treeNext).toEqual({ value: null, done: false });
      tree.abort();
      expect(await treeChanges.next()).toEqual({ value: undefined, done: true });

      git(path, "add", "file.txt");
      expect(await next).toEqual({ value: ["index"], done: false });
      next = changes.next();
      writeFileSync(join(path, ".git", "info", "exclude"), "build/\n");
      git(path, "commit", "-q", "-m", "First");
      expect((await next).value).toContain("refs");

      controller.abort();
    },
  );

  it("reports refs stored in a reftable", { timeout: 10_000 }, async () => {
    // What `git init --ref-format=reftable` (git 2.45+) writes to, instead of `refs`.
    const path = createRepo("watched-reftable");
    mkdirSync(join(path, ".git", "reftable"));
    const repo = await repos.open("watched-reftable");
    const controller = new AbortController();
    const changes = watchGitDir(repo, controller.signal);

    const next = changes.next();
    soon(() => writeFileSync(join(path, ".git", "reftable", "tables.list"), "x"));
    expect(await next).toEqual({ value: ["refs"], done: false });

    controller.abort();
  });

  it("ends quietly when aborted while starting", async () => {
    createRepo("watched-abort");
    const repo = await repos.open("watched-abort");
    const ends = [watchGitDir, watchWorkingTree].map((watch) => {
      const controller = new AbortController();
      const next = watch(repo, controller.signal).next();
      controller.abort();
      return next;
    });
    expect(await Promise.all(ends)).toEqual([
      { value: undefined, done: true },
      { value: undefined, done: true },
    ]);
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
