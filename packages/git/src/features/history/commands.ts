import { stat } from "node:fs/promises";
import { resolve } from "node:path";

import { GitError } from "../../core/errors";
import type { Repo } from "../../core/repo";
import { LOG_FORMAT, parseLog } from "./parse";
import type { Commit } from "./schema";

export async function getLog(
  repo: Repo,
  page: { limit: number; skip: number },
  signal?: AbortSignal,
): Promise<Commit[]> {
  await updateCommitGraph(repo);
  // Only refs a user would recognise; `--all` also picks up tool namespaces (e.g. refs/t3/*).
  // HEAD is included for detached checkouts, unless the branch has no commits yet.
  const revisions = [
    "--branches",
    "--remotes",
    "--tags",
    ...((await repo.hasHead()) ? ["HEAD"] : []),
  ];
  const output = await repo.read(
    [
      "log",
      "-z",
      LOG_FORMAT,
      "--date-order",
      "--decorate=full",
      `--max-count=${page.limit}`,
      `--skip=${page.skip}`,
      ...revisions,
      "--",
    ],
    { signal },
  );
  return parseLog(output);
}

export async function getCommit(repo: Repo, sha: string, signal?: AbortSignal): Promise<Commit> {
  const args = ["log", "-z", LOG_FORMAT, "--decorate=full", "--max-count=1", sha, "--"];
  const [commit] = parseLog(await repo.read(args, { signal }));
  // `sha` names a tree or blob, say, rather than a commit.
  if (!commit) throw new GitError(`${sha} is not a commit.`, args, 0, "");
  return commit;
}

/**
 * Each repository's latest commit-graph write, and whether it's still waiting for the one before
 * it to finish.
 */
const commitGraphWrites = new Map<string, { done: Promise<void>; waiting: boolean }>();
/** Each repository's commit-graph chain file, once looked up. */
const commitGraphChains = new Map<string, string>();

/**
 * Brings the repository's commit-graph up to date. Without one, `--date-order` walks the whole
 * history before git shows the first commit: seconds, with 100k+ commits. The same goes for the
 * commits it doesn't have yet, so it's updated before every read, not just written once.
 *
 * With `--split`, an update adds a layer with only the new commits, which takes milliseconds, so the
 * log waits for it. The first write takes seconds in a big repository, and so does the first one
 * after `git gc` replaced the layers with a single file; the log doesn't wait for those.
 */
async function updateCommitGraph(repo: Repo): Promise<void> {
  try {
    const incremental = await exists(await commitGraphChain(repo));
    const write = writeCommitGraph(repo);
    if (incremental) await write;
  } catch {
    // Only an optimisation; the log works without it.
  }
}

function writeCommitGraph(repo: Repo): Promise<void> {
  const latest = commitGraphWrites.get(repo.path);
  // A write that hasn't started yet will see the newest commits too.
  if (latest?.waiting) return latest.done;
  // One that has may have started before them, so another one follows it.
  const write = { done: Promise.resolve(), waiting: true };
  write.done = (latest?.done ?? Promise.resolve())
    .then(async () => {
      write.waiting = false;
      // Not a `write`: it leaves the index and refs alone and takes a lock of its own, so it
      // needn't wait for (or hold up) staging and committing.
      await repo.read(["commit-graph", "write", "--reachable", "--split"]);
    })
    .catch(() => undefined)
    .finally(() => {
      if (commitGraphWrites.get(repo.path) === write) commitGraphWrites.delete(repo.path);
    });
  commitGraphWrites.set(repo.path, write);
  return write.done;
}

async function commitGraphChain(repo: Repo): Promise<string> {
  let chain = commitGraphChains.get(repo.path);
  if (chain === undefined) {
    const path = await repo.read([
      "rev-parse",
      "--git-path",
      "objects/info/commit-graphs/commit-graph-chain",
    ]);
    chain = resolve(repo.path, path.trim());
    commitGraphChains.set(repo.path, chain);
  }
  return chain;
}

function exists(path: string): Promise<boolean> {
  return stat(path).then(
    () => true,
    () => false,
  );
}
