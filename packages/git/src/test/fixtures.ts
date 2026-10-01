// Real repositories in a temp directory, for tests that run git. Each test file gets its own
// directory (Vitest loads this module once per file), removed once the file's tests are done.
import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import type { RepositoryService } from "@gitto/repository/server";
import { ORPCError } from "@orpc/server";
import { afterAll, expect } from "vitest";

import { GitReposImpl, type Repo } from "../core/repo";

export const root = mkdtempSync(join(tmpdir(), "gitto-git-"));

afterAll(() => {
  rmSync(root, { recursive: true, force: true });
});

/** Repository paths by id, for `repos`. */
export const paths = new Map<string, string>();

/** Opens the repositories `createRepo` made, by their name. */
export const repos = new GitReposImpl({
  getRepository: async (id: string) => {
    const path = paths.get(id);
    return path ? { id, name: id, path } : undefined;
  },
} as RepositoryService);

/** Runs git in `cwd` and returns its trimmed output. */
export function git(cwd: string, ...args: string[]) {
  return execFileSync("git", args, { cwd, encoding: "utf8" }).trim();
}

/** Creates an empty repository on `main`, which `repos` opens as `name`, and returns its path. */
export function createRepo(name: string): string {
  const path = join(root, name);
  git(root, "init", "-q", "-b", "main", name);
  git(path, "config", "user.name", "Test User");
  git(path, "config", "user.email", "test@example.com");
  git(path, "config", "commit.gpgsign", "false");
  paths.set(name, path);
  return path;
}

/**
 * A repository with some history: `first`, then a `side` branch that renames `b.txt` and adds
 * `s.txt`, merged into `main` (tagged `v1`) after a commit there, plus a commit only a tool's ref
 * points at. `a file.txt` is modified and `new file.txt` untracked.
 */
export async function createHistoryRepo(name = "history"): Promise<Repo> {
  const path = createRepo(name);
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
  return repos.open(name);
}

/** The first page of the log. */
export const page = { limit: 50, skip: 0 };

/** What `promise` rejects with; fails if it resolves. */
export function rejection(promise: Promise<unknown>): Promise<unknown> {
  return promise.then(
    () => expect.fail("expected a rejection"),
    (reason: unknown) => reason,
  );
}

/** The API error `promise` rejects with; fails if it resolves or rejects with something else. */
export async function apiError(promise: Promise<unknown>): Promise<unknown> {
  const error = await rejection(promise);
  expect(error).toBeInstanceOf(ORPCError);
  return error;
}
