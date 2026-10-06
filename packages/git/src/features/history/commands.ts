import { GitError } from "../../core/errors";
import type { Repo } from "../../core/repo";
import { LOG_FORMAT, parseLog } from "./parse";
import type { Commit } from "./schema";

/** A page of the history over every branch, remote and tag, newest first. */
export async function getLog(
  repo: Repo,
  page: { limit: number; skip: number },
  signal?: AbortSignal,
): Promise<Commit[]> {
  const hasHead = await repo.hasHead();
  // Only refs a user would recognise; `--all` also picks up tool namespaces (e.g. refs/t3/*).
  // HEAD is included for detached checkouts, unless the branch has no commits yet.
  const revisions = ["--branches", "--remotes", "--tags", ...(hasHead ? ["HEAD"] : [])];
  const read = async (order: string[]) =>
    parseLog(
      await repo.read(
        [
          "log",
          "-z",
          LOG_FORMAT,
          ...order,
          "--decorate=full",
          `--max-count=${page.limit}`,
          `--skip=${page.skip}`,
          ...revisions,
          "--",
        ],
        { signal },
      ),
    );

  // To sort them with children first, git walks back to the oldest commit a ref points at first:
  // 0.5s on vscode, whose old remote branches go back years, 4.3s on linux, and seconds more
  // without a commit-graph, as in a fresh clone. Unsorted, it hands them over by date as it comes
  // across them, in 40-50ms on both, which is the same order unless a commit is dated before its
  // parent. So that's read first, and only sorted by git if a commit did end up below one of its
  // parents. That's remembered, so the unsorted log isn't read in vain every time after.
  //
  // Only the first page: the ones after it are skipped to in git's order, so they follow on from
  // each other whichever way the first was read.
  let commits = page.skip > 0 || repo.sortsLog() ? undefined : await read([]);
  if (commits && !childrenFirst(commits)) {
    repo.sortLog();
    commits = undefined;
  }
  commits ??= await read(["--date-order"]);
  // For when it does need sorting. After the log, not alongside it: the first write can take
  // seconds, and would slow it down.
  void repo.updateCommitGraph();
  return commits;
}

/** Whether every commit comes before its parents, as the history's graph needs them. */
function childrenFirst(commits: Commit[]): boolean {
  const listed = new Set<string>();
  for (const commit of commits) {
    if (commit.parents.some((parent) => listed.has(parent))) return false;
    listed.add(commit.sha);
  }
  return true;
}

/** The commit `sha`, as the history lists it. */
export async function getCommit(repo: Repo, sha: string, signal?: AbortSignal): Promise<Commit> {
  const args = ["log", "-z", LOG_FORMAT, "--decorate=full", "--max-count=1", sha, "--"];
  const [commit] = parseLog(await repo.read(args, { signal }));
  // `sha` names a tree or blob, say, rather than a commit.
  if (!commit) throw new GitError(`${sha} is not a commit.`, args, 0, "");
  return commit;
}
