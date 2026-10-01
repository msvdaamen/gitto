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
  void writeCommitGraph(repo);
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

/** Repositories `writeCommitGraph` has looked at since the app started. */
const commitGraphChecked = new Set<string>();

/**
 * Writes the repository's commit-graph, in the background, if it doesn't have one. Without it,
 * `--date-order` walks the whole history before git shows the first commit: seconds, with 100k+
 * commits; with it, git only walks about as far as the commits it shows. Git writes one itself on
 * `gc` and `maintenance`, but a fresh clone, say, has none yet.
 */
async function writeCommitGraph(repo: Repo): Promise<void> {
  if (commitGraphChecked.has(repo.path)) return;
  commitGraphChecked.add(repo.path);
  try {
    // A single file, or a chain of them in a folder (`git commit-graph write --split`).
    const output = await repo.read([
      "rev-parse",
      "--git-path",
      "objects/info/commit-graph",
      "--git-path",
      "objects/info/commit-graphs",
    ]);
    const paths = output.split("\n").filter(Boolean);
    const found = await Promise.all(
      paths.map((path) =>
        stat(resolve(repo.path, path)).then(
          () => true,
          () => false,
        ),
      ),
    );
    if (found.includes(true)) return;
    // Not a `write`: it leaves the index and refs alone and takes a lock of its own, and in a big
    // repository it would hold up staging and committing for seconds.
    await repo.read(["commit-graph", "write", "--reachable"]);
  } catch {
    // Only an optimisation; the log works without it.
  }
}
