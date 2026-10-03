// Stands in for `@/lib/rpc` in the perf suites: there's no main process outside Electron, so this
// serves `server.repository` instead, the way the main process would serve a real one.
import type { ChangedFile, Commit, Ref, Stash, Uncommitted } from "@gitto/git/types";
import type { RpcClient } from "@gitto/rpc/client";

import { fakeRepository, random, type FakeRepository } from "./repository";

type Repository = Awaited<ReturnType<RpcClient["repository"]["list"]>>[number];

export const REPOSITORY: Repository = {
  id: "0199a3b4-0000-7000-8000-000000000000",
  name: "gitto",
  path: "/home/ada/gitto",
};

/** What the fake main process serves; a suite replaces it before rendering the app. */
export const server: { repository: FakeRepository } = { repository: fakeRepository(200) };

type Options = { signal?: AbortSignal };

function uncommitted(): Uncommitted {
  const main = server.repository.refs.find((ref) => ref.current)!;
  return {
    head: { kind: "branch", name: main.name, sha: main.sha },
    upstream: main.upstream,
    ahead: 0,
    behind: 0,
    counts: { files: 0, staged: 0, unstaged: 0, conflicted: 0 },
    changes: { staged: [], unstaged: [] },
    version: "1",
  };
}

/** The files a commit changed: a handful to a few dozen, the same ones every time. */
function commitFiles(sha: string): ChangedFile[] {
  const next = random(Number.parseInt(sha.slice(0, 8), 16));
  return Array.from({ length: 1 + Math.floor(next() * 40) }, (_, i) => ({
    path: `src/${["components", "features", "lib"][i % 3]}/module-${Math.floor(next() * 100)}/file-${i}.ts`,
    status: next() < 0.8 ? "modified" : "added",
    origPath: null,
    additions: Math.floor(next() * 200),
    deletions: Math.floor(next() * 100),
  }));
}

/** A watch that never reports a change, until it's stopped. */
async function quiet(options?: Options): Promise<AsyncIterable<never>> {
  const stopped = new Promise<never>((_, reject) =>
    options?.signal?.addEventListener("abort", () => reject(options.signal?.reason)),
  );
  return { [Symbol.asyncIterator]: () => ({ next: () => stopped }) };
}

function findCommit(sha: string): Commit {
  const found = server.repository.commits.find((commit) => commit.sha === sha);
  if (!found) throw new Error(`No commit ${sha}`);
  return found;
}

export const rpc = {
  repository: {
    list: async (): Promise<Repository[]> => [REPOSITORY],
  },
  git: {
    history: {
      log: async (): Promise<Commit[]> => server.repository.commits,
      commit: async ({ sha }: { sha: string }): Promise<Commit> => findCommit(sha),
    },
    status: {
      get: async ({ since }: { since?: string }) =>
        since === uncommitted().version ? { unchanged: true as const } : uncommitted(),
    },
    refs: { list: async (): Promise<Ref[]> => server.repository.refs },
    stash: {
      list: async (): Promise<Stash[]> => [],
      files: async (): Promise<ChangedFile[]> => [],
    },
    diff: {
      commitFiles: async ({ sha }: { sha: string }): Promise<ChangedFile[]> => commitFiles(sha),
    },
    commit: {
      message: async ({ sha }: { sha: string }): Promise<string> => findCommit(sha).subject,
      pushedTo: async (): Promise<string | null> => "origin/main",
    },
    watch: {
      gitDir: (_input: unknown, options?: Options) => quiet(options),
      workingTree: (_input: unknown, options?: Options) => quiet(options),
    },
  },
};
